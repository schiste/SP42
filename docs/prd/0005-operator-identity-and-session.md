# PRD-0005: Operator identity and session

**Author:** Luis Villa
**Date:** 2026-06-05
**State:** Implemented
**As-built:** retroactive characterization of an already-shipped feature (no forward "closing PR").
**Related ADRs:** ADR-0002 (local dev-auth bridge contract)
**Discussion:** (PR link added on filing)

## Problem

An operator who wants to act on Wikimedia through SP42 — patrol a change, roll
back vandalism, undo or tag an edit — must do so *as a specific Wikimedia
account*, and only with the rights that account actually holds on the target
wiki. Before this feature, the browser had no trustworthy way to establish "who
am I acting as, and what may I do here." Two failure modes had to be avoided:

- The browser holding a raw Wikimedia access token. SP42's UI compiles to Wasm
  and runs in the operator's browser; a token living there is a credential in
  the wrong place.
- The browser *asserting* its own identity, scopes, or expiry. If the client
  could name the username or claim the `rollback` scope, the action surface
  would be gated on a self-reported claim rather than on what Wikimedia
  actually grants.

This feature is for the single local operator running SP42 against a live wiki:
it establishes a server-owned session that fixes the operator's Wikimedia
identity and derives — from the real token and the wiki's own answer — which
moderation and editing actions that identity may take. ADR-0002 owns the
structural bridge contract; this PRD owns the operator-facing intent: *what
identity I act as, and what that identity is allowed to do.*

## Proposal

SP42 gives the operator a server-owned authenticated session. The browser is an
interface, never the credential store.

- **Establishing identity.** In local mode the operator starts a session by
  posting an empty bootstrap request; the server reads the Wikimedia token from
  `.env.wikimedia.local` and derives the authenticated **username**, the
  effective **scopes**, and the session **expiry** from the token itself. The
  operator cannot name their own username, hand themselves scopes, or set their
  own expiry — those are server-derived
  (`crates/sp42-server/src/auth_routes.rs:259` `post_bootstrap_session`;
  `crates/sp42-server/src/oauth_runtime.rs:187` `validate_bootstrap_payload`).
- **What the browser sees.** The browser only ever receives **session state** —
  authenticated yes/no, username, the derived scope list, expiry, bridge mode,
  and a per-session CSRF token. It never receives the Wikimedia access token;
  the session report says `token_present: true` without disclosing the value
  (`crates/sp42-server/src/session_runtime.rs:110` `to_status`).
- **What identity I am allowed to do.** Against the target wiki SP42 produces a
  **capability report** for the operator's identity: it probes the OAuth grants
  on the token, the account's **groups** and **rights** on that wiki, and
  whether Wikimedia will issue the matching action **tokens** (csrf, patrol,
  rollback). An action is allowed only when *all three* line up — OAuth grant
  AND wiki right AND a real action token. So `can_edit`, `can_patrol`, and
  `can_rollback` reflect what this account can actually do *on this wiki right
  now*, not merely what the token was issued for
  (`crates/sp42-server/src/wikimedia_capabilities.rs:261` `derive_report`).
- **Capability gates the action surface.** The same capability report gates both
  the UI and the server. In the operator's action bar, the rollback, undo, and
  patrol buttons are disabled when the identity lacks the matching capability
  (`crates/sp42-app/src/components/action_bar.rs:62`). On the server every
  action request is re-checked against the live capability report before it is
  executed, so a missing right is refused with a clear reason even if the UI is
  bypassed (`crates/sp42-server/src/action_routes.rs:762`
  `validate_action_request`).
- **Acting and ending the session.** Every state-changing action and the logout
  carry the session's CSRF token; an action request also requires an active
  session. Logout clears the server session and expires the cookie, returning
  the operator to an unauthenticated, `inactive` state
  (`crates/sp42-server/src/action_routes.rs:40` `post_execute_action`;
  `crates/sp42-server/src/auth_routes.rs:227` `post_auth_logout`).
- **Where this is offered.** The local-token bootstrap is a single-operator
  local-development affordance and is available only in `local` deployment
  mode; it is refused in `vps` and `desktop` modes
  (`crates/sp42-server/src/deployment.rs:32` `permits_dev_token_bootstrap`).
  A browser OAuth login path exists for non-local identity establishment
  (`crates/sp42-server/src/auth_routes.rs:31` `get_auth_login`,
  `:178` `get_auth_callback`).

## Definition of Done

Each item is a behavior that is already true in the shipped code, bound to an
existing test.

- [x] The server derives the operator's username, scopes, and expiry from the
  validated local token (the operator cannot supply them) — verified by
  `crates/sp42-server/src/tests.rs::bootstrap_derives_username_and_scopes_from_validated_token`.
- [x] A bootstrap request that tries to set its own username, scopes, or expiry
  is rejected with `400` — verified by
  `crates/sp42-server/src/tests.rs::bootstrap_rejects_caller_supplied_identity_scope_and_expiry`.
- [x] The bootstrap request body is canonically empty on the wire, so the
  browser cannot smuggle identity fields — verified by
  `crates/sp42-core/src/dev_auth.rs::tests::builds_bootstrap_request` and
  `::bootstrap_request_ignores_legacy_fields_when_serializing`.
- [x] Session state reported to the browser flags `token_present` without
  disclosing the access-token value — verified by
  `crates/sp42-server/src/tests.rs::to_status_hides_token_value`.
- [x] A capability report for an identity treats an action as available only
  when the OAuth grant, the wiki right, and the action token all line up — a
  `rollback` grant without the wiki `rollback` right yields `can_rollback =
  false` (with an explanatory note) — verified by
  `crates/sp42-server/src/wikimedia_capabilities.rs::tests::derives_rollback_as_unavailable_without_wiki_right`.
- [x] The capability route returns the derived per-wiki capability report
  (`can_patrol`, `can_rollback`, etc.) for the operator's identity — verified by
  `crates/sp42-server/src/tests.rs::capability_route_uses_injected_targets`.
- [x] The local-token bootstrap is refused outside `local` deployment mode with
  a `403` naming `SP42_DEPLOYMENT_MODE=local` — verified by
  `crates/sp42-server/src/tests.rs::bootstrap_session_is_disabled_outside_local_mode`,
  and the mode predicate is exercised by
  `crates/sp42-server/src/deployment.rs::tests::local_mode_is_the_only_dev_token_mode`.
- [x] Clearing the session requires the session CSRF token; a cookie-bearing
  `DELETE` without it is refused `403` and the session survives, with it the
  session is removed — verified by
  `crates/sp42-server/src/tests.rs::dev_session_delete_requires_csrf_for_cookie_session`.
- [x] A bootstrapped session round-trips end to end — bootstrap establishes the
  identity (`username = "Schiste"`, `bridge_mode = "local-env-token"`), and a
  CSRF-bearing logout returns the operator to `authenticated = false` /
  `bridge_mode = "inactive"` with a `Max-Age=0` clearing cookie — verified by
  `crates/sp42-server/tests/operator_live.rs::auth_logout_clears_bootstrapped_session_state`.
- [x] In `vps` mode the session cookie is marked `Secure` (and always
  `SameSite=Lax`) — verified by
  `crates/sp42-server/src/tests.rs::vps_session_cookie_is_secure`.
- [x] The legacy client-asserted session installer (`PUT /dev/auth/session`) is
  not available; only the derive-from-token bootstrap path exists — verified by
  `crates/sp42-server/src/tests.rs::put_session_is_disabled_for_single_user_local_token_path`.

## Alternatives

- **Token in the browser.** Rejected by ADR-0002: the browser is an interface,
  not a credential store. The session view deliberately omits the access token
  and reports only `token_present`
  (`crates/sp42-server/src/session_runtime.rs:110`).
- **Browser-asserted identity, scopes, and expiry.** An earlier shape let the
  client install a session by `PUT`ing a username/scopes/expiry. The shipped
  design removed it — the bootstrap payload is canonically `{}` and any
  non-empty identity field is rejected — so the action surface is gated on what
  Wikimedia grants, never on a self-report
  (`crates/sp42-core/src/dev_auth.rs:18`,
  `crates/sp42-server/src/oauth_runtime.rs:187`).
- **Trusting OAuth grants alone for capability.** A token carrying the
  `rollback`/`patrol` grant does not mean the account holds that right on a
  given wiki. The shipped capability derivation requires grant AND wiki right
  AND a live action token, and explains the gap when a grant is present but the
  right is missing (`crates/sp42-server/src/wikimedia_capabilities.rs:294`).
- **One auth path everywhere.** The local-env-token bootstrap is a
  single-operator local convenience and is confined to `local` mode; the
  browser OAuth login path covers other deployments
  (`crates/sp42-server/src/deployment.rs:32`).

## Risks

- **Stale capability vs. live wiki rights.** The action surface is derived from a
  capability probe; if an account's rights changed since the probe, the UI could
  briefly show a stale affordance. Mitigation: the server re-validates every
  action request against a capability report at execution time and refuses with
  a specific reason, so the gate is enforced server-side regardless of UI state
  (`crates/sp42-server/src/action_routes.rs:52`).
- **Session left open.** A bootstrapped session persists until cleared or it
  expires. Mitigation: the session carries idle (30 min) and absolute (8 h)
  expiry deadlines and is pruned on access, and logout clears it immediately
  (`crates/sp42-server/src/session_runtime.rs:197`,
  `crates/sp42-server/src/main.rs:113`). Note: the expiry deadline logic itself
  is not directly unit-tested (see Known gaps).
- **CSRF on state-changing calls.** Actions and logout require the per-session
  CSRF token, mitigating cross-site request forgery against the localhost
  server; the negative path (refusal without the token) is tested
  (`crates/sp42-server/src/tests.rs::dev_session_delete_requires_csrf_for_cookie_session`).
- **Local token file as a secret.** The operator's Wikimedia token lives in
  `.env.wikimedia.local`; protecting that file is the operator's
  responsibility. The server keeps it server-side and never echoes it to the
  browser, but cannot protect the file on disk.

## Known gaps / drift

Factual observations from reverse-engineering the shipped code, not design
questions.

- **The browser OAuth round-trip is not integration-tested.** The PKCE/OAuth
  helpers in `crates/sp42-core/src/oauth.rs` are well unit-tested (state,
  challenge, URL build, callback parse, state-mismatch rejection), but the
  server-side login → callback → session-install path
  (`get_auth_login`/`get_auth_callback`/`complete_auth_callback`, the
  `wikimedia-oauth` bridge mode) has no end-to-end test. Only the
  `local-env-token` bootstrap path is exercised end to end
  (`crates/sp42-server/tests/operator_live.rs:459`).
- **`effective_session_scopes` derivation is not directly tested.** The mapping
  from a capability report to the `["basic","editpage","patrol","rollback"]`
  scope list (`crates/sp42-server/src/session_runtime.rs:12`) is only observed
  transitively through the bootstrap test's asserted scope list; there is no
  unit test pinning the mapping itself.
- **Session expiry logic is not unit-tested.** `session_expires_at_ms` /
  `session_is_expired` / `prune_expired_sessions`
  (`crates/sp42-server/src/session_runtime.rs:197`) and the idle/absolute
  timeout constants have no dedicated test asserting that an idle or aged
  session actually expires.
- **The OAuth login precondition and redirect sanitization are untested.** The
  `412` returned when the confidential OAuth client is not configured
  (`crates/sp42-server/src/auth_routes.rs:36`) and `sanitize_redirect_target`'s
  open-redirect guard (`crates/sp42-server/src/oauth_runtime.rs:16`) have no
  verifying test.
- **Capability gating of the action route is tested only at the unit level.**
  `validate_action_request` (`crates/sp42-server/src/action_routes.rs:762`)
  encodes the per-action capability gate, but no test drives
  `post_execute_action` to confirm a capability-lacking request is refused
  through the HTTP route; the in-module test only covers action-feedback
  formatting.
- **Identity-derived scopes vs. raw OAuth scope fall back inconsistently.** When
  the capability probe succeeds, scopes come from the derived capabilities;
  when it does not, the session falls back to the token's raw OAuth grants or
  scope string (`crates/sp42-server/src/auth_routes.rs:131`). This fallback (a
  session whose scope list reflects granted-but-unverified scopes) is not
  characterized by a test.
- **Two adjacent session-status shapes.** `OAuthSessionView` (browser-facing,
  uses `FlagState` enums) and `DevAuthSessionStatus` (returned by the bootstrap
  and `/dev/auth/session` routes) carry overlapping identity/session fields with
  different encodings; the operator-facing meaning is the same but there are two
  representations to keep in sync.
