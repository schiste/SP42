# PRD-0017: Interactive review sessions — agent↔operator feedback loop on a page

**Drafter:** Claude Code (Fable)
**Editor:** Luis Villa
**Date:** 2026-07-11
**State:** Draft
**Discussion:** <PR link TBD>
**Spawned ADRs:** [ADR-0018](../adr/0018-review-session-bridge-contract.md)
(review-session bridge contract and store placement)

## Problem

An agent working on citations (via `sp42-cli` or `sp42-mcp`) and an operator
reviewing an article currently have no shared, structured feedback loop. The
operator's judgment reaches the agent as free-form chat: "the third paragraph's
quote isn't in the source" — no page identity, no revision, no block, no cite
id. The agent guesses at anchors, and the operator repeats themselves.

Local-first agent review tools solved this loop for *files*:
[lavish-axi](https://github.com/kunchenguid/lavish-axi) keys a session to an
HTML artifact's canonical path, lets the human annotate elements and text
ranges in a browser, and delivers the queued, anchored prompts to whichever
agent polls the session. The loop etiquette it ships — deliver queued feedback
before reporting an ended session, refuse to reopen a session the *human*
ended unless explicitly asked, tell the agent its next step in every response
— is what makes the loop usable by agents without bespoke prompting.

SP42 wants that loop, keyed to what SP42 reviews: a **wiki page**, identified
by `(wiki_id, title)` and pinned to a revision — passing a Wikipedia URL where
lavish-axi passes a file path.

## Proposal

A localhost **review-session bridge**: the agent opens a session on a page
target (bare title or pasted wiki URL, including `oldid` URLs), the operator
queues anchored feedback prompts against that page, and the agent polls the
session to collect them.

User-facing behavior:

- **Open**: `sp42-cli review open <title-or-URL>` opens (or resumes) the
  session and returns a compact **article outline** — block ordinals, block
  kinds, truncated text, and cite ids from the Parsoid decomposition — the
  agent's map for resolving anchors, plus a `next_step` telling it to poll.
- **Annotate**: the operator queues prompts anchored to a block ordinal,
  optionally narrowed to a cite id (`cite_ref-…`) or a verbatim selected-text
  range, or free-form messages. Anchors use Parsoid structure, not CSS
  selectors, so they survive re-rendering and map directly onto the
  use-site anchors the verification domain already uses. The MVP queueing
  surface is `sp42-cli review queue` (dev/test) and the gated
  `/dev/review/prompts` route; the browser annotation panel is a committed
  follow-up (see Open questions).
- **Poll**: `sp42-cli review poll <title-or-URL>` waits for feedback. Each
  server request is a bounded wait; the CLI re-arms until feedback, an end,
  or a missing session, narrating the wait on stderr while stdout stays a
  single structured response. `--agent-reply "<summary>"` posts a chat line
  to the operator surface before the wait starts.
- **Findings overlay**: the session doubles as the *in-article frontend for
  the verification report*. `sp42-cli review findings <title-or-URL>
  --report <verify-page-json>` attaches a `PageVerificationReport`'s
  findings to the session, projected onto the same anchors prompts use —
  each finding joins its outline block by cite id, carrying the verdict, a
  truncated claim, and a caveat (dead source, ungrounded support). The next
  open returns the annotated outline, so the operator sees where the
  problems are in the article instead of reading a detached text report,
  and can queue prompts directly against them. Markers matching no block
  surface as `unanchored_findings`, never silently dropped; a report
  produced against a different revision than the session's pin is refused
  (`review-findings-revision-mismatch`). Attaching spends no inference —
  verification stays operator-triggered (PRD-0014 posture); the agent runs
  `verify-page` explicitly and hands the result over.
- **Session-end etiquette** (ported): feedback queued before an end still
  delivers first, flagged `session_ended`; only a later poll reports `ended`.
  An **operator**-ended session refuses a plain reopen (HTTP 409 with
  guidance) unless `--reopen` is passed; an **agent**-ended session
  (`sp42-cli review end`) reopens freely. An ended session also refuses new
  prompts (`review-session-ended`) until it is explicitly opened again — a
  stale queue must not flip a closed session back to feedback around the
  gate. Every response carries `next_step` prose telling the agent what to
  do — poll again, apply-and-reply, or stop without reopening.
- **Inventory**: `sp42-cli review sessions` and `GET /dev/review/sessions`
  list open sessions with pending-prompt counts.

Everything stays read-only with respect to the wiki: a review session never
edits anything. Acting on feedback rides the existing operator-confirmed
lanes (re-verify, bare-URL repair, inline edit).

## Definition of Done

- [ ] Opening with a pasted wiki URL unwraps the title and records the pinned
  revision, verified by `review_session_loop_delivers_operator_feedback_to_the_agent`
  (`sp42-server`)
- [ ] Prompts queued before an operator "send & end" deliver on the next poll
  flagged `session_ended`, and only the following poll reports `ended`,
  verified by `feedback_queued_before_an_end_delivers_before_ended`
  (`sp42-platform`) and the server loop test
- [ ] A plain open of an operator-ended session refuses with
  `review-session-operator-ended` and an explicit `reopen` resumes it,
  verified by `review_reopen_gate_requires_explicit_reopen_after_operator_end`
  (`sp42-server`)
- [ ] Feedback drains exactly once — a second poll after delivery reports
  `waiting`, verified by `take_feedback_drains_once_and_reopens`
  (`sp42-platform`)
- [ ] Queueing wakes a concurrently waiting poll without a full timeout wait,
  verified by `queued_feedback_wakes_a_waiting_poll` (`sp42-server`)
- [ ] All review POST routes require an authenticated bridge session and CSRF
  header, verified by `review_routes_require_an_authenticated_bridge_session`
  (`sp42-server`)
- [ ] Every response embeds `contract_version` and a `next_step`, and the
  ended `next_step` distinguishes operator-ended (do not reopen) from
  agent-ended, verified by `next_step_matches_the_loop_etiquette`
  (`sp42-platform`)
- [ ] Attached findings overlay the next open's outline joined by cite id,
  with unmatched markers surfaced (not dropped), verified by
  `annotate_outline_joins_findings_onto_blocks_by_ref_id` (`sp42-platform`)
  and `review_findings_attach_and_overlay_the_next_open` (`sp42-server`)
- [ ] A report for a different revision than the session's pin refuses with
  `review-findings-revision-mismatch`, and resuming to a new revision drops
  the stale overlay, verified by `attach_findings_refuses_a_revision_mismatch`
  and `resume_to_a_new_revision_drops_stale_findings` (`sp42-platform`)
- [ ] Report findings project onto review anchors — verdict wire labels,
  truncated claims, grounding/unavailable caveats, ref-less standalone
  findings skipped — verified by
  `review_finding_markers_project_the_report_onto_review_anchors`
  (`sp42-citation`)
- [ ] Queueing to an ended session refuses with `review-session-ended`, and
  agent chat replies are readable back on the open response, verified by
  `queueing_to_an_ended_session_is_refused` (`sp42-platform`) and
  `review_queue_refuses_an_ended_session_and_replies_surface_in_open`
  (`sp42-server`)
- [ ] Session opens, feedback queueing, findings attaches, deliveries, and
  ends emit `tracing` events, checkable in server logs

## Alternatives

- **Adopt lavish-axi itself** (open the article as a local HTML artifact):
  rejected — its identity model (canonical file path, chokidar file watching,
  sibling-asset serving) does not map to remote wiki URLs; its annotation
  anchors (truncated CSS selectors + DOM ranges) are weaker than Parsoid
  block/cite anchors for wiki work; and SP42 already owns the localhost
  server, session/CSRF runtime, and Parsoid read path the loop needs.
- **WebSocket feedback channel** (reuse the coordination-room substrate):
  deferred — a long-poll matches the agent's blocking-command ergonomics
  (one CLI invocation = one rendezvous) and needs no client protocol. The
  browser annotation panel may later ride the existing WebSocket/SSE paths.
- **Key sessions by URL string** (hash the raw URL like lavish-axi hashes
  paths): rejected — two URLs for the same page (percent-encoding,
  underscores, `oldid` forms) must resolve to one session, so the canonical
  key is the parsed `(wiki_id, title)` pair.

## Risks

- **The MVP has no browser annotation surface**, so operator queueing runs
  through the CLI/route until the panel ships — the loop is real but the
  operator ergonomics are not yet. Mitigation: the wire contract is versioned
  and panel-agnostic; the browser panel consumes the same routes.
- **Feedback is held in server memory only**; a server restart drops queued
  prompts, unlike lavish-axi's `state.json`. Acceptable for a localhost dev
  bridge; revisit persistence when multi-operator review sessions arrive.
- **An agent could ignore the reopen etiquette** and hammer `reopen: true`.
  The gate is advisory by design (same as the ported tool); the operator
  surface always shows who reopened.

## Open questions

1. **Where does the operator annotate?** Proposed: a Review panel in the
   browser shell's article surface, listing outline blocks — with attached
   findings rendered inline as per-block badges — plus per-block queue
   actions and a session composer, reusing the `/dev/review` routes.
   Until then the CLI `review queue` command is the queueing surface.
2. **MCP surface**: should `sp42-mcp` grow `review_open`/`review_poll` verbs?
   Proposed: yes, but the MCP server is standalone today (no localhost-server
   client); bridging it is its own small ADR.
