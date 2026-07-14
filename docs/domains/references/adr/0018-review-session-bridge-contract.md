# ADR-0018: Review-session bridge contract and store placement

**Status:** Proposed
**Date:** 2026-07-11
**Author:** Claude Code (Fable), for Luis Villa

PRD-0017 ports the agent↔human artifact-review loop popularized by
[lavish-axi](https://github.com/kunchenguid/lavish-axi) from local HTML files
to wiki pages. This ADR records the structural decisions: where the contract
lives, how sessions are keyed, how the long-poll is implemented, and how the
routes are gated.

## Context

The ported loop has three parties — an agent (CLI/MCP), an operator surface
(browser), and a rendezvous server — and one contract: session state plus a
queued-feedback drain with strict etiquette (deliver-before-ended, operator
ends gate reopens, `next_step` guidance in every response). lavish-axi keys
sessions by canonical file path, persists them in `~/.lavish-axi/state.json`,
watches the file for live reload, and long-polls one indefinite HTTP request
with whitespace heartbeats. None of the file-specific machinery maps to a
remote wiki page; the loop semantics map cleanly.

SP42 constraints that shaped the port: contracts shared by more than one crate
live in `sp42-platform` (ADR-0013); business logic is pure and tested with no
I/O (Constitution Art. 1–2); the localhost server's mutable stores are
`Arc<RwLock<HashMap>>` fields on `AppState` with session+CSRF gating on
mutating routes (ADR-0002); external interfaces carry versioned serde
contracts (Art. 9.1).

## Decision

1. **Pure core in `sp42-platform::review_session`.** `ReviewSession` owns
   every transition — `queue_prompts` (which refuses an ended session so a
   stale queue cannot flip it back to feedback around the reopen gate),
   `take_feedback` (deliver-before-ended, drain-exactly-once), `gate_reopen`
   (operator-ended refuses a plain reopen; the route checks this against
   the stored session *before* any remote revision/Parsoid read), `resume`,
   `end`, `agent_reply` (chat is echoed back on the open response so
   replies are not write-only) — as pure methods over injected `now_ms`. The wire types (`ReviewOpenRequest/Response`, `ReviewPollRequest/
   Response`, `ReviewQueueRequest/Response`, ack/list shapes) live beside
   them, all embedding `REVIEW_SESSION_CONTRACT_VERSION`. The `next_step`
   strings are pure functions too, so the loop etiquette is unit-tested.

2. **Session identity is the parsed `(wiki_id, title)` pair.** The open
   route accepts a bare title or a pasted wiki URL and unwraps it with the
   existing `parse_page_target` (same as verify-page), so every URL spelling
   of a page collapses to one session — the analog of lavish-axi's
   canonical-path key. The store key is `wiki_id ␟ title` (unit-separator
   joined, so the pair is unambiguous), and `canonical_key` applies the
   wiki-independent half of MediaWiki title normalization to the title
   component — underscores are spaces, whitespace runs collapse — so bare
   titles, URL spellings, and direct API callers all key one session.
   First-letter case is deliberately *not* folded: case rules are
   wiki-dependent (Wiktionary titles are case-sensitive), so folding could
   merge two genuinely distinct pages; the residual cost is that a
   lowercase-typed title opens a separate session on case-insensitive
   wikis. Full API-side normalization (case, namespace aliases) would need
   a wiki round-trip per request and is out of scope for the dev bridge.
   The revision is session *state* (pinned at open, re-pinned on resume),
   not identity.

3. **Server store: `AppState.review_sessions`, in-memory, with a per-session
   `tokio::sync::Notify`.** Each entry pairs the pure `ReviewSession` with a
   `Notify`; queue/end call `notify_one`, whose stored permit covers feedback
   queued while no poll is waiting. The poll handler drains, waits
   (`tokio::time::timeout` over `notified()`, clamped to 55 s), and drains
   once more; an omitted `wait_ms` uses the server default, while an
   explicit `wait_ms: 0` skips the wait — a nonblocking status check. Bounded server-side waits with a re-arming CLI loop replace
   lavish-axi's single indefinite request — no heartbeat protocol, no
   proxy-timeout exposure, identical agent ergonomics. No cross-restart
   persistence: this is a localhost dev-bridge surface (PRD-0017 Risks).

4. **Routes under `/dev/review/*`, POSTs session+CSRF gated.** Path constants
   in `sp42_platform::routes` like every other route contract. All six POST
   routes (`open`, `prompts`, `poll`, `findings`, `agent-reply`, `end`) require the
   bridge session cookie plus the CSRF header (ADR-0002) — open reads through
   the caller's wiki identity to pin the latest revision, and the rest mutate
   session state. `GET /dev/review/sessions` is an ungated read-only
   inspection surface, listed in the operator endpoint manifest.

5. **The article outline replaces the DOM snapshot.** Where lavish-axi ships
   a walked-DOM outline with synthetic uids, the open response ships
   `build_article_outline` over the Parsoid blocks the editor already
   extracts (`extract_blocks`, ADR-0011): block ordinal, kind, truncated
   text, cite ids. Prompt anchors reuse the same coordinates
   (`block_ordinal` + optional `ref_id` + optional verbatim `selected_text`),
   which map directly onto citation use-sites — no CSS selectors.

6. **Agent surface is `sp42-cli review …` over the bridge-bootstrap pattern.**
   Open/poll/queue/reply/end/sessions subcommands authenticate exactly like
   `verify-page` (bootstrap → cookie + CSRF token). The poll loop keeps
   stdout reserved for one structured response and narrates waiting on
   stderr, mirroring the ported tool's agent ergonomics. MCP verbs are
   deferred (PRD-0017 open question 2) because `sp42-mcp` has no
   localhost-server client today.

7. **The verification report overlays the outline as citation-agnostic
   markers.** `sp42-platform` gains `ReviewFindingMarker` — `ref_id`,
   verdict wire label, truncated claim, optional caveat — deliberately *not*
   the citation domain's `CitationFinding`: the platform layer never depends
   on report types (ADR-0013 layering), so the citation crate projects its
   report into markers at the edge (`review_finding_markers`, which also
   translates grounding caveats and unavailable reasons into prose and skips
   ref-less standalone findings). Markers attach via `POST
   /dev/review/findings` with replace-all semantics (one attach = one
   verification run) and a revision gate — a report pinned to a different
   revision returns 409 `review-findings-revision-mismatch`, and resuming a
   session onto a new revision drops the stale overlay. On open, the pure
   `annotate_outline` joins markers onto blocks by `ref_id` (the same
   coordinate prompts anchor to); unmatched markers return as
   `unanchored_findings` rather than dropping. Attaching wakes no poll —
   findings are operator-surface input, not agent feedback.

## Consequences

- The loop's semantics are testable without a server, and the server tests
  exercise only glue (gating, wake-ups, status codes).
- A browser Review panel needs no new contract — it consumes the same routes
  the CLI queueing surface uses.
- The text report is no longer the only operator surface for verification
  results: any consumer of the open response gets the report as an
  in-article, per-block overlay for free.
- Session state dies with the server process; if review sessions outgrow the
  dev bridge (multi-operator, cross-restart), persistence and eviction become
  a follow-up ADR alongside the deployment-mode gating question.
- The bounded-wait poll means worst-case feedback latency for a poll that
  raced a notification is one wait window; the permit semantics make that
  race a non-event in the single-agent case.
