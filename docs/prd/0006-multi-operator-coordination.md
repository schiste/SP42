# PRD-0006: Multi-operator coordination

**Author:** Luis Villa
**Date:** 2026-06-05
**State:** Implemented
**As-built:** retroactive characterization of an already-shipped feature (no forward "closing PR").
**Related ADRs:** ADR-0001 (foundational decisions — coordination server hosting and the trait-isolated I/O posture)
**Discussion:** (PR link added on filing)

> **As-built note.** This PRD documents coordination behavior that already ships
> and is covered by tests; it is not a forward requirement. The Definition of
> Done is re-framed as *characterization*: every checked item is a behavior that
> is already true and bound to an existing test. Things that are absent,
> inconsistent, or only partly wired are recorded under **Known gaps / drift**,
> not papered over. In particular, read that section first for the boundary
> between what the *server relay and protocol* do (fully built and tested) and
> what the *interactive patrol UI* does (display-only today).

## Problem

When two reviewers work the same recent-changes queue, they can land on the same
revision at the same moment — both opening the diff, both deciding to roll back
or mark patrolled — and collide: duplicated work at best, conflicting actions at
worst. A reviewer working a busy wiki has no way to see *who else* is currently
looking at a revision, or whether someone has already taken it. SP42 needed a
coordination surface so a room of operators on the same wiki can see each other's
presence and claims and avoid stepping on one another.

The shipped feature provides the server-side relay and the shared, deterministic
room state that make this possible, plus REST surfaces and a debug panel that let
an operator (or a developer running the local operator shell) observe the live
collaboration picture for a wiki: who is connected, who is present on edits, which
revisions are claimed, and what actions have just been taken. It is the
collaboration spine for local multi-operator development; the interactive
in-patrol-UI gestures that would sit on top of it are not yet wired (see Known
gaps).

## Proposal

For a given wiki (a coordination *room*, keyed by `wiki_id`), SP42 lets operators
share a live collaboration picture over a single WebSocket per room:

- **Presence** — an operator announces that they are actively reviewing, with a
  count of how many edits they have open (a *presence heartbeat*). Other operators
  in the room see that operator appear in the room's presence list; when the count
  drops to zero, the operator clears from presence. Presence that goes silent is
  pruned server-side after a staleness window so a crashed or vanished reviewer
  does not linger as "present."
- **Claims** — an operator claims a specific revision (an *edit claim*) so others
  know it is being handled. The room tracks one claimant per revision; a later
  claim on the same revision takes over (last-writer-wins) until a *race
  resolution* names the authoritative winner, after which the room pins that
  winner.
- **Live fan-out** — every claim, presence change, action, score delta, flagged
  edit, and race resolution one operator emits is relayed to every *other*
  connected operator in the same room (the sender does not receive its own
  message back), and folded into the shared room state.
- **Identity is the session, not the wire** — when a message arrives over an
  authenticated socket, the server overwrites the message's actor with the
  authenticated operator's username before relaying it, so one operator cannot
  attribute a claim, presence, action, or race-resolution to someone else.
- **Catch-up for late joiners** — a room's accumulated state (current claims,
  present operators, recent actions, race resolutions) is readable over REST, so
  an operator who connects late, or a debug panel, can recover the current picture
  without having watched the live stream. A freshly connected socket is *not*
  replayed the backlog; it reads state via the inspection endpoint instead.
- **Rooms are self-cleaning** — a room with no connected clients and no activity
  is evicted after an idle window, so the relay does not accumulate dead rooms.

Today this picture is surfaced to operators as a **read-only collaboration
narrative** in the bootstrap snapshot and the debug/inspector panel (e.g.
`active_actors`, `claimed_revisions`, and a derived collaboration mode such as
`active`, `claimed`, `contested`, or `under-review`). The interactive gesture of
*claiming from inside the patrol view* is part of the client protocol library but
is not yet wired to a UI control (see Known gaps).

## Definition of Done

Each item is a behavior that is already true, bound to an existing test.

- [x] Operator messages fan out to every *other* operator in the same room, and
  claim, presence, action, and race-resolution all round-trip across three
  authenticated live WebSocket clients — verified by
  `sp42-server/src/tests.rs::multi_user_coordination_flow_round_trips_across_authenticated_clients`.
- [x] The server overwrites a message's actor with the authenticated operator's
  session username before relaying it (a client sending actor `"Mallory"` is
  relayed and recorded as the real session user) — verified by
  `sp42-server/src/tests.rs::multi_user_coordination_flow_round_trips_across_authenticated_clients`
  (the wire actor `"Mallory"` arrives at peers and lands in room state as
  `"Alice"`/`"Bob"`/`"Carol"`).
- [x] A presence heartbeat with a positive edit count makes the operator present;
  a heartbeat of zero clears them from the room's presence list — verified by
  `sp42-server/src/tests.rs::anonymous_multi_user_flow_preserves_actor_and_clears_presence`
  and, at the reducer level, `sp42-core/src/coordination_state.rs::removes_presence_when_active_count_hits_zero`.
- [x] Competing claims on the same revision follow last-writer-wins, and a race
  resolution then pins the winning operator so subsequent claims by others no
  longer take over that revision — verified by
  `sp42-server/src/tests.rs::competing_claims_follow_last_writer_until_race_resolution`
  and, at the reducer level,
  `sp42-core/src/coordination_state.rs::aggregates_score_deltas_and_applies_race_resolution`.
- [x] An operator who connects late recovers the current claims, present
  operators, and race-resolution state of a room via the inspection endpoint
  without being replayed the live backlog — verified by
  `sp42-server/src/tests.rs::fresh_client_recovers_race_resolved_state_via_room_inspection`
  (which also asserts the late joiner receives no replay).
- [x] Presence that goes silent past the staleness window is pruned from the
  room's reported state even while the operator's socket stays connected —
  verified by `sp42-server/src/tests.rs::stale_presence_is_pruned_from_room_state_reports`.
- [x] Room state (claims, presence) survives an operator disconnect and is
  re-observed on reconnect; connected-client counts stay correct across a
  reconnect storm — verified by
  `sp42-server/src/tests.rs::reconnecting_client_resubscribes_and_room_state_persists`,
  `sp42-server/src/tests.rs::coordination_room_persists_after_disconnect_and_reports_zero_clients`,
  and `sp42-server/src/tests.rs::reconnect_storm_keeps_room_counts_and_live_delivery_consistent`.
- [x] An undecodable coordination payload is still relayed to peers but is counted
  as invalid and does not mutate room state — verified by
  `sp42-server/src/tests.rs::invalid_coordination_payload_is_counted_without_mutating_state`.
- [x] The room snapshot, room-state, room-inspection, and inspection-collection
  REST endpoints serve the live coordination picture — verified by
  `sp42-server/src/tests.rs::coordination_snapshot_route_is_available`,
  `::coordination_room_state_route_is_available`,
  `::coordination_room_inspection_route_is_available`,
  `::coordination_inspections_route_is_available`, and
  `::missing_coordination_room_inspection_returns_empty_bootstrap_model`.
- [x] Coordination messages survive a MessagePack encode/decode round-trip for
  every message kind — verified by
  `sp42-core/src/coordination_codec.rs::property_round_trip_identity` (proptest).
- [x] The debug/inspector panel renders the room into a collaboration narrative
  (active actors, claimed revisions, and a derived mode such as `active` /
  `contested`) rather than only raw counts — verified by
  `sp42-app/src/platform/coordination.rs::coordination_room_narrative_lines_surface_collaboration_details`
  and `::room_inspection_lines_cover_presence_and_state`.

## Alternatives

- **Persistent / cross-instance room state.** Room state is held in memory in a
  single server process (`CoordinationRegistry`, an `Arc<RwLock<HashMap>>`). The
  shipped shape trades durability for simplicity, matching the local-development
  posture (README: multi-user production auth is not implemented yet). A shared
  store (Redis, a database) was not built; the design implies it was deferred
  until coordination graduates beyond local dev.
- **Replaying the backlog to new sockets.** A late joiner could have been caught
  up by replaying the room's message history over the socket. Instead the design
  keeps the socket replay-free and exposes accumulated state over REST
  (`/coordination/rooms/{wiki_id}` and `…/inspection`), so catch-up is a single
  read rather than a stream the client must reconcile. The fresh-client test
  asserts no replay is sent.
- **Trusting the client-supplied actor.** The wire format carries an actor field,
  which a naive relay would trust. The shipped server rewrites it from the
  authenticated session on every relevant message kind, so attribution cannot be
  spoofed on an authenticated connection — at the cost of trusting the actor
  verbatim on *anonymous* connections (the intended local-dev behavior; see
  Risks).
- **Explicit claim release.** A dedicated "release my claim" message was not
  added; the design relies on claim hand-off (a newer claim or a race resolution)
  and whole-room idle eviction to clear stale claims. This keeps the message set
  small at the cost of stale-claim lifetime (see Known gaps).

## Risks

- **Stale claims mislead operators.** Because there is no claim-release message,
  a reviewer who claims a revision and then leaves can keep that revision marked
  as claimed until someone re-claims it, a race resolution lands, or the room goes
  fully idle (5-minute eviction). *Mitigation that exists:* presence is pruned
  independently after 60s of silence, so the *who-is-present* picture self-heals
  even when the *who-claimed-what* picture does not. *Residual:* an operator may
  defer to a claim whose owner is long gone.
- **State is volatile.** All coordination state lives in one process's memory; a
  server restart wipes every room's claims and presence. *Mitigation:* none in
  code — operators reconnect and rebuild the picture from new activity. Acceptable
  for the current local-development scope.
- **Anonymous attribution is unverified.** On an unauthenticated socket the
  client-supplied actor is trusted as-is. *Mitigation that exists:* on
  authenticated sockets the server overwrites the actor from the session, so any
  deployment that requires auth gets trustworthy attribution; the anonymous path
  is the documented local-dev posture, not a production stance.
- **The collaboration picture is observe-only in the GUI today.** An operator can
  *see* presence and claims in the debug/inspector surface but cannot *claim* or
  *announce presence* from inside the patrol view, because the live socket client
  is not wired to a UI control. *Mitigation:* none needed for local development;
  flagged as drift below so the boundary is not mistaken for a finished
  collaboration UX.

## Known gaps / drift

Factual observations from reverse-engineering the shipped code; these replace
"Open questions."

- **The live socket client is not wired into the interactive patrol UI.** The
  browser app fetches only a read-only bootstrap snapshot
  (`sp42-app/src/platform/bootstrap.rs:29`); the in-app fetch helpers are retained
  only as dead-code keep-alive references
  (`let _ = coordination::fetch_coordination_snapshot;` … `sp42-app/src/lib.rs:92-99`).
  No UI, desktop, or CLI call site invokes `CoordinationClient::claim_edit` or
  `send_presence` (`sp42-core/src/coordination_client.rs:36,97`), and the
  `CoordinationRuntime` that couples client transport to live state
  (`sp42-core/src/coordination_runtime.rs`) has no caller outside its own unit
  tests. So claiming an item or emitting presence is **not reachable through the
  shipped GUI**; only the server relay and the client protocol library are built
  and tested.
- **CLI / desktop "coordination" previews are fixtures.** Both replay a hardcoded
  `coordination_preview_messages()` set (`sp42-cli/src/main.rs:1997`;
  `build_coordination_snapshot`, `sp42-desktop/src/main.rs:579`) to demonstrate the
  codec and reducer, not live room data.
- **No explicit claim release / un-claim.** `CoordinationMessage`
  (`sp42-core/src/types.rs:481`) has no release variant. A claim changes hands only
  via a later `EditClaim` (last-writer-wins) or a `RaceResolution`, and is
  otherwise dropped only when the whole room is idle-evicted after 5 minutes
  (`ROOM_IDLE_EVICT_AFTER_MS`, `sp42-server/src/coordination.rs:13`).
- **Presence staleness is tested via a test hook, not real time.** The 60s
  presence timeout (`PRESENCE_STALE_AFTER_MS`,
  `sp42-server/src/coordination.rs:12`) is exercised by forcing the last-seen
  timestamp through `set_presence_last_seen_for_test`
  (`sp42-server/src/tests.rs:2504`). The clock-driven *room* eviction path is
  unit-tested in the registry (`evicts_idle_rooms_with_no_connected_clients`,
  `sp42-server/src/coordination.rs:472`).
- **Rooms are in-memory and per-process.** `CoordinationRegistry` holds an
  `Arc<RwLock<HashMap>>` (`sp42-server/src/coordination.rs:16`); state is lost on
  restart and is not shared across server instances. No persistence test exists
  because there is no persistence.
- **Actor rewriting is asymmetric across message kinds.** The server rewrites the
  actor for `ActionBroadcast`, `EditClaim`, `PresenceHeartbeat`, and
  `RaceResolution`, but passes `ScoreDelta` and `FlaggedEdit` through unchanged
  (`other => other`, `sp42-server/src/main.rs:1488`). Those two carry no actor
  field, so it is benign, but the asymmetry is undocumented.
- **Anonymous connections trust the wire actor.** With no session the
  sanitizer returns the payload unchanged (`sp42-server/src/main.rs:1464`),
  verified intentionally by
  `anonymous_multi_user_flow_preserves_actor_and_clears_presence`
  (`sp42-server/src/tests.rs:2155`). This is the local-dev posture, not a
  production guarantee.
- **Recent-actions log is silently capped at 25.** The reducer drains the oldest
  entries past 25 (`sp42-core/src/coordination_state.rs:134`); the cap is not
  surfaced to operators.
- **Score-delta reasons accumulate unbounded.** Merging deltas on one revision
  concatenates reasons with `\" | \"` (`sp42-core/src/coordination_state.rs:117`)
  with no length bound; no test guards reason growth.
- **Toolforge WebSocket support is an open hosting question.** ADR-0001 flags that
  persistent WebSocket support on Toolforge is unverified, with a VPS fallback
  (`docs/adr/0001-foundational-decisions.md:47`). The relay is tested against an
  in-process axum server, not a real deployment target.
