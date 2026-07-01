# ADR-0015: Rules-compliant read-only fetch edge

**Status:** Proposed
**Date:** 2026-06-29
**Author:** Luis Villa

SP42 fetches two kinds of URL while verifying citations: the **arbitrary
cited-source URL** (any host on the internet, and attacker-influenceable, since
citation URLs come from wiki content) and the **Wikimedia REST API** (the Citoid
metadata endpoint). Issue #34 asked for an ADR to settle the robustness +
safety policy of that fetch edge before it grew further; #51 already closed the
two most acute holes (redirect re-checking, a streaming size cap) and #60
tracks a remaining DNS-rebinding gap. This ADR records the decisions for the
whole edge and folds in #60, because the chosen mechanism closes it for free
rather than as separate work.

The headline decision is deliberately *un*-ambitious about new code: keep all of
reqwest's transport, redirect, TLS, and connection-pooling behavior; lean on
maintained middleware for retry; and add only the one small policy layer
(SSRF) that a general-purpose HTTP client cannot supply for us. The change is
net code *removed*, not added.

## Context

### What exists today, and why it needs consolidating

There are **two near-duplicate guarded source clients**, both implementing the
`sp42-types::HttpClient` trait:

- `CliHttpClient` (`sp42-cli`) — a manual redirect loop that re-checks each hop's
  URL against the SSRF floor, plus a streaming 8 MB cap.
- `PlainHttpClient` (`sp42-server`) built on `guarded_source_client`
  (`sp42-inference`) — the same floor + cap, but via a reqwest *custom redirect
  policy* closure instead of a manual loop.

They reimplement the same security-critical mechanics two different ways, across
three crates, and the SSRF predicate (`check_fetchable_source_url`,
`is_blocked_ipv4/v6` in `sp42-core`) hand-rolls its own CIDR classification.
This duplication is exactly the kind that drifts — and has (the per-hop check
and the cap were added to each client separately). Citoid currently rides the
*source* client, so a Wikimedia API call goes through the SSRF floor and the
cap.

What's genuinely missing (verified in the code): **retry/backoff + `Retry-After`
handling** (a single `.send()` today), and **resolved-IP SSRF validation** (the
floor checks URL *strings*, never the resolved address — #60). Timeouts and a
compliant User-Agent already exist and only need codifying.

### Threat model (and its limits)

The fetch edge's untrusted input is any URL whose **host derives from wiki
content** — the source body fetch. The realistic attack is a *spray*, not a
targeted hit: SP42-server processes the recent-changes / patrol firehose
unattended, so an attacker needs only to add a citation URL to *any* watched
page — e.g. `http://169.254.169.254/…` (cloud metadata) or `http://10.0.0.5/`
(an internal service) — and wait for the server to fetch it from inside its
own network. The harm is a **confused deputy**: SP42 can reach link-local /
internal addresses the attacker cannot, and is tricked into doing so on their
behalf. The high-value payoff is theft of the VM's IAM credentials if the
fetched body becomes observable (note ADR-0011's report echoes a located
*passage from the source*); the floor payoff is blind internal recon.

**This threat is proportionate, not urgent.** Three honest qualifications shape
the decision:

1. **Today the chain is dead.** SP42-server runs local-only — no cloud, no
   metadata endpoint, no internal services of value. Current real risk ≈ 0. The
   reliability symptoms below are the *actual* present-day problem.
2. **A modern deployment mostly neutralizes it at the infra layer.** IMDSv2
   (token + hop limit) makes a single blind SSRF GET insufficient to obtain
   credentials; an egress firewall blocking RFC1918 / link-local from the fetch
   service kills the internal-service variant. These deploy-layer controls are
   the *primary* mitigation and are substitutable for the in-code guard — if one
   can guarantee every deployment is hardened.
3. **The cited-source fetcher is the only untrusted edge.** The Citoid host is
   hardcoded (`en.wikipedia.org`); the attacker influences only the path param,
   not the host — so there is no SSRF vector on the Wikimedia call to defend.

Separately, several #34 items are **reliability**, not security, and earn their
place regardless of any attacker — a live run once hung indefinitely on a slow
source (`sfplanning.org`) until killed; that is the timeout/size-cap/redirect-cap
story.

### Why SSRF protection is ours to write at all

A general-purpose HTTP client cannot block private IPs by default: the same
request is correct in one program (talking to your own `localhost` / internal
mesh) and a vulnerability in another (fetching user-supplied URLs). The library
has no view of *trust context*; only the application does. So reqwest — like
Python `requests`, Go `net/http`, Node `fetch` — ships the **hooks**
(`dns_resolver`, `redirect::Policy`) and leaves the **policy** to the app. SSRF
protection ships built-in only in purpose-built "fetch untrusted URLs"
libraries, of which Rust has no mature one. The small layer we add is precisely
that purpose-built layer over reqwest-the-transport — not a reimplementation of
it.

"Validating the resolved IP" here is **not** authentication or reputation (that
would be intractable and is what the DNS trust chain is for). It is a purely
local range classification — `ip_network::is_global()`, a handful of bitmask
comparisons against ~15 RFC-defined reserved ranges, O(1), zero network. DNS
cannot do it for us: an attacker *legitimately and authentically* publishes
`A evil.com → 169.254.169.254`; DNSSEC would validate that record perfectly.
The "badness" is relative to *our* network position, knowledge that exists only
in our process.

## Decision

1. **One read-only fetch edge, in a new dep-light `sp42-fetch` crate.** It owns
   the guarded `HttpClient` over reqwest and is depended on by `sp42-cli` and
   `sp42-server`. A dedicated crate (not `sp42-core`) keeps reqwest / retry /
   `ip_network` out of the pure-domain crate (ADR-0004's anti-god-crate rule),
   and a dedicated crate (not `sp42-inference`, the only existing reqwest home)
   avoids forcing the CLI fetch to depend on the model crate — `CliHttpClient`
   deliberately holds no model bearer, and that property must survive. The
   `HttpClient` trait stays in `sp42-types`. This deletes both duplicate
   `execute` implementations in favor of one.

2. **Trust is expressed by which resolver is attached, not by separate client
   types.** A single `build_fetch_client(...)` factory builds the reqwest client;
   the **untrusted source face** attaches the guarded resolver, the **trusted
   Wikimedia face** attaches the default resolver. No host allowlist to
   misconfigure, no second implementation to drift. The target shape is that
   Citoid uses the trusted Wikimedia face (its host is hardcoded, so the SSRF
   guard would add only the risk of a self-inflicted outage on a first-party
   dependency, with no threat reduction). The current migration keeps the
   verify-page and CLI Citoid calls on the guarded source injection rather than
   widening those signatures in this PR; that is an explicit temporary exception,
   not a second policy.

3. **SSRF via a custom `Resolve` (resolved-IP guard), closing #60.** The guarded
   resolver resolves the host (using the normal system resolver — we do not
   reimplement DNS), keeps only addresses for which `ip_network::is_global()`
   holds, and hands reqwest *only* those. reqwest then connects to exactly the
   validated addresses, on the initial request **and every redirect hop** —
   which closes the resolve-then-connect TOCTOU window (#60) as a side effect.
   `ip_network` replaces the hand-rolled `is_blocked_ipv4/v6` CIDR logic. A small
   IP-literal range check remains as belt-and-suspenders, since the resolver is
   not guaranteed to fire on literal-IP hosts (no name to resolve).

4. **Redirects: a slimmer custom policy, not a manual loop.** Replace the CLI
   manual loop and the `guarded_source_client` closure with a `reqwest`
   redirect `Policy::custom` that caps hops at 5 **and rejects any hop whose
   target is a non-public IP literal**. DNS-name hops are validated by the
   resolver (#3) at connect time; literal-IP hops are *not* (reqwest skips DNS
   for them), so the per-hop literal check must stay in the redirect policy — a
   plain `Policy::limited(5)` would reopen the redirect-to-private-literal case.
   The initial URL gets the same literal check pre-flight in `execute`. (This is
   simpler than the old per-hop full-`check_fetchable_source_url` string check,
   but it is *not* zero.)

5. **Retry/backoff as a small in-`execute` loop (not `reqwest-retry`).** The
   original plan was to use `reqwest-retry` + `reqwest-middleware`; the build
   showed that crate **cannot honor `Retry-After`** (its policy sees only the
   retry count, its strategy only classifies responses), which the Wikimedia REST
   side wants. So the edge instead runs a ~30-line retry loop in `execute`: retry
   idempotent GETs on transient statuses (`408/429/5xx`), up to 3 attempts,
   waiting the server's **`Retry-After`** when present (capped) else exponential
   backoff with full jitter; **never** retry transport/connect errors (a
   deterministic SSRF rejection surfaces as one and must fail fast — Implementation
   note 3). This *reverses* the "lean on the maintained crate" instinct, but it
   is the only way to honor `Retry-After`, and it has the side benefit of
   **dropping two dependencies** (`reqwest-retry`, `reqwest-middleware`) and the
   reqwest-0.12 version pin — leaving `ip_network` as the only genuinely new
   external crate (`rand`, for jitter, is already in the workspace).

6. **`maxlag` is explicitly out of scope.** It applies to the Action API
   (`api.php`) only (since MediaWiki 1.27) and returns HTTP 200 + `Retry-After`;
   Citoid is the REST API, which `maxlag` does not touch. Recorded so it is not
   re-added by mistake. REST politeness is covered by #5 plus low concurrency
   (≤ 3 concurrent, per Wikimedia REST guidance) when verifying a whole article.

7. **Size + redirect caps retained.** Keep the 8 MB streaming body cap (reqwest
   has no built-in knob; this stays as ~minimal streamed-`chunk` code, now in one
   place) and the 5-hop redirect cap (#4), applied to both faces.

8. **Timeouts codified, not invented.** `connect_timeout(10s)` + total
   `timeout(30s)` (the values already in use), applied per attempt; retry wraps
   them. Constants for now; promote to config only if a need appears.

9. **User-Agent unchanged.** `SP42/0.1.0 (+https://github.com/.../SP42)` is
   compliant with the Wikimedia UA policy — a URL contact is sufficient; no email
   is required. Kept as the single shared constant.

10. **Dev/test escape hatch preserved, single source of truth.**
    `SP42_FETCH_ALLOW_PRIVATE=1` (for the loopback-serving benchmark harness)
    swaps the guarded resolver for a pass-through one, read once in the factory
    rather than at three independent call sites.

11. **Deploy-layer controls are the primary modern mitigation; the in-code guard
    is defense-in-depth.** The ADR records IMDSv2 + egress filtering as the
    expected production hardening. The in-code resolver is justified because it
    does not depend on every future deployment being hardened, it is ~30 lines,
    and #60 is a logged P1 the maintainers want closed in code.

12. **Testability without a new heavy dependency.** Unit-test the guarded
    resolver with injected resolutions (no network). Integration-test SSRF the
    way #51 already does — raw loopback servers + reqwest `.resolve()` so a
    floor-passing hostname can reach the test server while the guard stays
    active — asserting redirect-to-private and literal-to-private are both
    blocked. Retry / `Retry-After` are tested against a local stub server. A
    record/replay (`ReplayHttpClient`) cassette is **not** adopted now; revisit
    only if hermetic full-fetch replay becomes necessary.

## Consequences

- **Code consolidated (measured — see Implementation notes).** Deletes the two
  duplicate `execute` bodies, the CLI manual redirect loop + `redirect_location`,
  the `guarded_source_client` redirect-policy closure + `redirect_host_allowed`,
  and the hand-rolled `is_blocked_ipv4/v6` CIDR logic — into one `sp42-fetch`
  crate. The implementation shows this is a **net reduction even excluding
  tests** (≈ −45 production lines — modest, not the −130/−150 first estimated),
  and ≈ −300 lines once duplicated tests collapse. The decisive win is
  de-duplication (4 files / 3 crates → 1). See the Implementation notes for why
  production code did not shrink as much as sketched.
- **New dependencies, and why they are not bloat:** exactly **one genuinely new
  external crate — `ip_network`** (IP range classification, replacing hand-rolled
  CIDR logic); `rand` (jitter) is already a workspace dependency. The
  `reqwest-retry` + `reqwest-middleware` crates were tried and then **dropped**
  (they cannot honor `Retry-After`; see decision #5), which also removed a
  reqwest-version pin. No new DNS stack — the resolver wraps the existing system
  resolver. Net: the codebase gets smaller *and* the external-dependency count
  rises by one.
- **Closes #34's open items** (retry/backoff, codified timeouts, confirmed UA;
  `maxlag` ruled out with reason) **and #60** (resolved-IP / DNS-rebinding) as a
  side effect of the resolver.
- **Future code avoided:** no hand-rolled backoff/jitter/`Retry-After` state
  machine, no separate #60 fix, no perpetual two-client sync.
- **Migration:** `sp42-cli` and `sp42-server` switch their source fetch to
  `sp42-fetch`. Bare-URL repair already uses a separate general client for
  Citoid; verify-page and CLI Citoid calls keep using the guarded injected
  client for now, which works because `en.wikipedia.org` is public. A strict
  trusted-face Citoid injection remains follow-up work; see Implementation notes
  on the two-face decision. Behavior is preserved or strengthened; the escape
  hatch and the report contract are unchanged.

## Alternatives considered

- **One client, always SSRF-guard (incl. Wikimedia).** Simplest, but applies the
  guard beyond the threat model (Citoid's host is not attacker-influenced) and
  makes a guard false-positive a self-inflicted outage on a first-party
  dependency. Rejected in favor of resolver-by-face (#2).
- **Two fully separate client types.** Matches "two rule sets" literally, but
  duplicating the shared transport recreates exactly the drift bug being fixed.
  The resolver-by-face split gets the type-level clarity with one transport.
- **Keep the URL-string floor only; skip resolved-IP checks.** Trivially
  bypassed (`A evil.com → 169.254.169.254` needs no rebinding *timing*), so it is
  close to security theater on its own. Rejected.
- **Hand-rolled retry/backoff.** ~100+ lines of security-adjacent state machine
  vs. a maintained crate. Rejected; `backoff` specifically is unmaintained
  (RUSTSEC-2025-0012).
- **Deploy-layer SSRF mitigation only (IMDSv2 + egress), no in-code guard.**
  Legitimate and arguably sufficient in a hardened modern deploy, but leaves #60
  open in code and depends on deploy discipline we cannot guarantee. Recorded as
  the primary *complementary* control (#11) rather than the sole one.
- **`sp42-core` or `sp42-inference` as the home.** Rejected: the former pulls
  reqwest/retry into the pure-domain crate; the latter forces the CLI fetch to
  depend on the model crate.

## Out of scope / non-goals

- **Malicious source *content*** (e.g. prompt injection in fetched text) is the
  verification/LLM layer's concern, not the transport edge's.
- **TLS/PKI** is reqwest default; not weakened.
- **RESTBase deprecation.** Citoid is reached via `en.wikipedia.org/api/rest_v1/`
  (RESTBase), which Wikimedia is actively sunsetting (T262315, T133001). An
  eventual migration to the `api.wikimedia.org` gateway (and the authenticated
  5,000/hr tier, if volume ever warrants) is noted but not undertaken here.

## References

- Issues: #34 (this edge), #60 (DNS-rebinding P1), #43 / PR #51 (redirect +
  size-cap hardening already landed), #25 (grounding-gate work that surfaced it).
- ADRs: ADR-0004 (crate boundaries), ADR-0011 (article-level verify path /
  report contract), ADR-0002 (local dev auth bridge — a `localhost` co-located
  service).
- External: Wikimedia UA policy; `Manual:Maxlag_parameter` (Action-API-only);
  Wikimedia APIs/Rate_limits (REST `429 + Retry-After`); RFC 9110 §9.2.2
  (GET idempotent); OWASP SSRF Prevention Cheat Sheet; `ip_network`,
  `reqwest-retry` crates.

## Implementation notes (ground-truth, 2026-06-29)

The decisions above were validated by building the crate and consolidating the
CLI, server, inference, and core (all tests green). Five things the build
corrected or surfaced — recorded so the design is honest:

1. **IP-literal hosts bypass the resolver — the resolver is *not* sufficient
   alone.** `reqwest` only runs a custom resolver for DNS *names*; an IP-literal
   host (initial URL *or* a redirect `Location`) connects directly. So the guard
   needs two companions the original sketch omitted: a **pre-flight literal
   check** in `execute` (initial URL) and a **custom redirect `Policy`** that
   rejects non-public literal hops (decision #4's "just `Policy::limited`" was
   wrong — we keep a custom policy, only simpler than the old per-hop string
   check). This was caught by a test: a literal `127.0.0.1` pointing at a live
   loopback server was fetched until the pre-flight check was added.

2. **`reqwest` discards the resolver's error detail.** A DNS-resolved SSRF
   rejection surfaces only as a generic "error sending request"; the specific
   reason cannot be recovered at the retry/caller layer (it *is* preserved for
   the literal pre-flight, which happens outside `reqwest`). Consequence for the
   verify-page report: a rebinding-style block is reported generically.

3. **Retry runs only on statuses, not transport errors (decision #5).** Because
   of (2), a deterministic SSRF rejection is indistinguishable from a transient
   connect failure at the retry layer — `reqwest-retry`'s default retried it
   `MAX_RETRIES` times (~2.6 s of wasted backoff). The shipped loop retries only
   on transient **HTTP statuses** (`408/429/5xx`) and never on transport/connect
   errors, so SSRF and dead hosts fail fast. Trade-off: genuine transient
   *connect* blips are not retried; the high-value retries (server-sent 503/429,
   and now `Retry-After`) are preserved.

4. **`reqwest-retry` was tried and dropped — replaced by a ~30-line loop.** It
   cannot honor `Retry-After` (decision #5) and forced a reqwest-0.12 version pin
   (its 0.9 line targets reqwest 0.13). Hand-rolling the loop honors `Retry-After`,
   removes both retry crates and the pin, and is verified by a timing test (a
   `Retry-After: 3` response makes the client wait ~3 s, not the sub-second
   default backoff). (`genai` still pulls a second `reqwest` 0.13 transitively —
   unrelated and harmless.)

5. **The two-face split is only partly realized in this migration.** Citoid is
   reached from two server paths, and they differ: the *bare-URL repair* path
   already uses the separate general `reqwest::Client`
   (`citation_routes::fetch_citoid_object`), but the *verify-page* path passes
   the single injected source client into `verify_page`, and
   `citation/verify.rs` uses that same client for the Citoid metadata request.
   So in verify-page (and in the CLI) Citoid currently rides the **guarded**
   face, not a trusted one. This works because `en.wikipedia.org` is public (the
   guard is a no-op there) and was preferred over threading a second client
   through the `verify_page`/`verify.rs` signature in this PR. A strict typed
   trusted face for all Citoid calls remains the follow-up to decision #2's
   target architecture.

6. **IPv4-mapped IPv6 must be unwrapped (Codex P1).** `Ipv6Network::is_global()`
   treats the whole `::ffff/96` block as global, so `http://[::ffff:127.0.0.1]/`
   (or a DNS answer of that form) reached loopback/metadata via the embedded
   IPv4. `is_public_ip` now unwraps `to_ipv4_mapped()` and classifies the
   embedded IPv4 first.

**LOC, measured (tests excluded, per the consolidation).** Production code is
≈ −45 net: ~383 prod lines removed (CLI `CliHttpClient`, server `PlainHttpClient`,
inference `guarded_source_client`/`redirect_host_allowed`, core CIDR helpers)
vs ~338 added (the `sp42-fetch` crate + wiring). The crate's production code came
in larger than the "lean on libraries" sketch precisely because of (1) and (3):
the literal pre-flight, custom redirect policy, and custom retry strategy are
hand-written. Counting tests, the net is ≈ −300 lines as duplicated SSRF tests
across four crates collapse into one focused suite. **The honest headline is
de-duplication + correctness (#60 closed, literal/redirect bypasses closed,
retry added), not a large raw-line reduction.**
