# PRD-0005: Operator identity and session

**Author:** Luis Villa
**Date:** 2026-06-05
**State:** Implemented
**As-built:** retroactive characterization of an already-shipped feature (no forward "closing PR").
**Related ADRs:** ADR-0002 (local dev-auth bridge — governs the *mechanism* of token custody, the canonical empty-bootstrap contract, and server-side identity/scope derivation; this PRD references that contract, it does not specify it)
**Discussion:** (PR link added on filing)

## Scope boundary

This PRD characterizes **what identity an operator acts as on the wiki**, **what that identity is allowed to do**, and **how the operator establishes and ends a session** — the user-facing meaning of "who am I acting as, and what may I do here." It deliberately excludes the structural plumbing behind that surface:

- **How identity is custodied and derived** — the server-owned session, the canonical empty bootstrap payload, deriving username/scopes/expiry from the real token, keeping the access token server-side, the per-session CSRF token, and the deployment-mode gating of the local-token path — is the local dev-auth bridge contract, **governed by ADR-0002**. This PRD states the operator-facing intent and links it; it does not describe the round-trip, the request/response shapes, or the token/CSRF/session-expiry internals.
- **Which dispositions that identity may then take, and how they execute** — is **PRD-0004** (reviewer actions). This PRD owns the capability surface that *gates* those dispositions; PRD-0004 owns the dispositions themselves.

## Problem

An operator who wants to act on Wikimedia through SP42 — patrol a change, roll back vandalism, undo or tag an edit — must do so *as a specific Wikimedia account*, and only with the rights that account actually holds on the target wiki. Before this feature the browser had no trustworthy way to establish "who am I acting as, and what may I do here," and two failure modes had to be avoided:

- **The browser holding the credential.** SP42's UI runs in the operator's browser; a raw Wikimedia access token living there is a credential in the wrong place.
- **The browser asserting its own identity.** If the client could name its own username or claim a `rollback` scope, the action surface would be gated on a self-report rather than on what Wikimedia actually grants.

This feature is for the single local operator running SP42 against a live wiki. It establishes an authenticated session that fixes the operator's Wikimedia identity and derives — from the real token and the wiki's own answer — which moderation and editing actions that identity may take. *How* that bridge is built is **ADR-0002**; this PRD owns the operator-facing intent: **what identity I act as, and what that identity is allowed to do.**

## Proposal

The operator acts under a server-owned authenticated session. The browser is an interface, never the credential store.

- **Whose identity I act as.** In local mode the operator starts a session and the server determines — from the real Wikimedia token — the authenticated **username**, the effective **scopes**, and the session **expiry**. The operator cannot name their own username, hand themselves scopes, or set their own expiry; those are server-derived, so the action surface is gated on what Wikimedia grants, never on a self-report. *(The empty-bootstrap contract and derivation are **ADR-0002**.)*
- **What I see of my session.** The browser only ever sees **session state** — authenticated yes/no, username, the derived scope list, expiry, and bridge mode. It never receives the Wikimedia access token; the session report says a token is present without disclosing its value.
- **What my identity is allowed to do.** Against the target wiki SP42 produces a **capability report** for the operator's identity. An action is allowed only when *all three* line up — the OAuth grant on the token, the account's right on that wiki, **and** a real action token Wikimedia will issue. So `can_edit`, `can_patrol`, and `can_rollback` reflect what this account can actually do *on this wiki right now*, not merely what the token was issued for: a `rollback` grant on a token, on a wiki where the account lacks the `rollback` right, reads as *not allowed*, with an explanatory note.
- **Capability gates the action surface.** The same capability report gates both what the operator sees and what the server will do. In the operator's action bar the rollback, undo, and patrol affordances are disabled when the identity lacks the matching capability; and every action request is independently re-checked server-side before it executes, so a missing right is refused with a clear reason even if the UI is bypassed. *(Which dispositions ride this gate is **PRD-0004**.)*
- **Establishing and ending the session.** Logout returns the operator to an unauthenticated, **inactive** state, clearing the session immediately. State-changing calls and logout are protected against cross-site forgery. *(The login round-trip, CSRF token, and session-expiry policy are **ADR-0002** / implementation.)*
- **Where the local path applies.** The local-token bootstrap is a single-operator local-development convenience, available only in `local` deployment mode and refused elsewhere; a browser OAuth login path covers non-local identity establishment. *(The mode gating is **ADR-0002** / implementation.)*

## Definition of Done

Each item is an operator-observable behavior that is **already true**, bound to an existing test. (The bridge *contract* itself — the empty bootstrap payload's serialization, token custody, and derivation plumbing — is mechanism owned by **ADR-0002** and is additionally unit-tested in `crates/sp42-core/src/dev_auth.rs` and `crates/sp42-server/src/`; those contract tests are noted here, not enumerated as user-facing DoD.)

- [x] The server fixes the operator's username, scopes, and expiry from the validated local token — the operator cannot supply them — verified by `crates/sp42-server/src/tests.rs::bootstrap_derives_username_and_scopes_from_validated_token`.
- [x] An attempt to self-assert a username, scopes, or expiry is rejected (`400`), so identity is never browser-claimed — verified by `crates/sp42-server/src/tests.rs::bootstrap_rejects_caller_supplied_identity_scope_and_expiry`, with the empty-on-the-wire contract pinned by `crates/sp42-core/src/dev_auth.rs::tests::builds_bootstrap_request` and `::bootstrap_request_ignores_legacy_fields_when_serializing`.
- [x] Session state shown to the operator reports a token is present without disclosing the access-token value — verified by `crates/sp42-server/src/tests.rs::to_status_hides_token_value`.
- [x] A capability reads as available only when the OAuth grant, the wiki right, and the action token all line up — a `rollback` grant without the wiki `rollback` right reads `can_rollback = false` with an explanatory note — verified by `crates/sp42-server/src/wikimedia_capabilities.rs::tests::derives_rollback_as_unavailable_without_wiki_right` (the three-way gate itself now lives in `crates/sp42-wiki/src/capabilities.rs::derive_wiki_capability_profile`, which that test exercises via `derive_report`).
- [x] The capability surface returns the derived per-wiki report (`can_patrol`, `can_rollback`, etc.) for the operator's identity — verified by `crates/sp42-server/src/tests.rs::capability_route_uses_injected_targets`.
- [x] The local-token bootstrap is refused outside `local` mode (`403`, naming `SP42_DEPLOYMENT_MODE=local`) — verified by `crates/sp42-server/src/tests.rs::bootstrap_session_is_disabled_outside_local_mode`, with the mode predicate exercised by `crates/sp42-server/src/deployment.rs::tests::local_mode_is_the_only_dev_token_mode`.
- [x] Ending the session is forgery-protected: a cookie-bearing clear without the session CSRF token is refused (`403`) and the session survives; with it the session is removed — verified by `crates/sp42-server/src/tests.rs::dev_session_delete_requires_csrf_for_cookie_session`.
- [x] A session round-trips end to end — bootstrap establishes the identity (`username = "Schiste"`, `bridge_mode = "local-env-token"`) and logout returns the operator to `authenticated = false` / `bridge_mode = "inactive"` — verified by `crates/sp42-server/tests/operator_live.rs::auth_logout_clears_bootstrapped_session_state`.
- [x] In `vps` mode the session cookie is marked `Secure` (and always `SameSite=Lax`) — verified by `crates/sp42-server/src/tests.rs::vps_session_cookie_is_secure`.
- [x] The legacy client-asserted session installer is gone; only the derive-from-token bootstrap path exists — verified by `crates/sp42-server/src/tests.rs::put_session_is_disabled_for_single_user_local_token_path`.

*(Which dispositions a capability gates — rollback / undo / patrol and the content edits — is owned by **PRD-0004**; the review workflow those affordances live in is **PRD-0002**; what a score MEANS, as distinct from this PRD's identity surface, is **PRD-0003**.)*

## Risks

*(Retroactive PRD — residual risks of the shipped behavior, with mitigations as
built; not a pre-implementation risk forecast. The mechanisms behind them are
ADR-0002 / implementation.)*

- **Stale capability vs. live wiki rights.** The action surface is derived from a capability probe; if an account's rights changed since the probe, the UI could briefly show a stale affordance. *Mitigation:* the server re-validates every action request against a capability report at execution time and refuses with a specific reason, so the gate holds server-side regardless of UI state.
- **Session left open.** A session persists until the operator logs out or it expires (the expiry policy is implementation, governed by ADR-0002). *Mitigation:* logout clears it immediately. *(The expiry timer itself is not directly tested — see Known gaps.)*
- **Forgery against the localhost server.** State-changing calls and logout require the per-session CSRF token. *Mitigation:* exists; the negative path (refusal without the token) is tested on the session-clear route.
- **Local token file as a secret.** The operator's Wikimedia token lives in `.env.wikimedia.local`; protecting that file is the operator's responsibility. The server keeps it server-side and never echoes it to the browser, but cannot protect the file on disk.

## Known gaps / drift

Factual observations from reverse-engineering the shipped code, not design questions. (These are coverage/drift gaps in a mechanism that **does** have a governing ADR — ADR-0002 — so, unlike PRD-0004, no missing *action-semantics* ADR is recorded here.)

- **The browser OAuth round-trip is not integration-tested.** The PKCE/OAuth helpers in `crates/sp42-core/src/oauth.rs` are well unit-tested (state, challenge, URL build, callback parse, state-mismatch rejection), but the server-side login → callback → session-install path (the `wikimedia-oauth` bridge mode) has no end-to-end test. Only the `local-env-token` bootstrap path is exercised end to end. ADR-0002 governs the local bridge but does not yet extend its contract to this browser-OAuth path.
- **The OAuth login precondition and redirect sanitization are untested.** The `412` returned when the confidential OAuth client is not configured (`crates/sp42-server/src/auth_routes.rs`), and the open-redirect guard on the callback's redirect target (`sanitize_redirect_target`, `crates/sp42-server/src/oauth_runtime.rs`), have no verifying test.
- **`effective_session_scopes` derivation is not directly tested.** The mapping from a capability report to the `["basic","editpage","patrol","rollback"]` scope list is a standalone function (`effective_session_scopes`, `crates/sp42-server/src/session_runtime.rs`) but is observed only transitively through the bootstrap test's asserted scope list; there is no unit test pinning the mapping itself.
- **Session expiry logic is not unit-tested.** The idle (30 min) and absolute (8 h) timeout policy and the prune-on-access behavior have no dedicated test asserting that an idle or aged session actually expires.
- **Capability gating of the action route is tested only at the unit level.** The per-action capability gate is encoded in `validate_action_request`, but no test drives the execute-action route to confirm a capability-lacking request is refused through HTTP. *(That route's coverage is shared with PRD-0004's Known gaps.)*
- **Identity-derived scopes vs. raw OAuth scope fall back inconsistently.** When the capability probe succeeds, scopes come from the derived capabilities; when it does not, the session falls back to the token's raw OAuth grants. This fallback — a session whose scope list reflects granted-but-unverified scopes — is not characterized by a test.
- **Two adjacent session-status representations.** The browser-facing session view and the bootstrap/dev-auth status shape carry overlapping identity/session fields with different encodings; the operator-facing meaning is the same, but there are two internal representations to keep in sync. (Internal shape, not operator-facing meaning.)
