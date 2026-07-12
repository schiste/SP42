# ADR-0023: Coordination contract — message kinds, relay, room state, wire codec

**Status:** Accepted
**Date:** 2026-07-11
**Author:** Luis Villa (drafted by Claude Code)
**Summary:** Multi-operator coordination is six message kinds relayed through a server-authoritative hub (authenticated actor rewriting, echo suppression, no backfill) and folded by one deterministic last-writer-wins-until-pinned reducer run identically on client and server, framed as MessagePack over an injectable `WebSocket` trait.

**As-built:** retroactive characterization of a shipped contract (PRD-0006).
Records the coordination message kinds, the relay/fan-out, room state, and the
wire codec as they exist, pinned by the reducer, codec, registry, and end-to-end
relay tests. Spawned from issue #21.

## Context

PRD-0006 (multi-operator coordination) documents the collaboration behavior —
operators seeing each other's presence, claims, and actions in real time — but
the structural contract had no ADR: the message kinds, how a message reaches
peers, what shared state a room holds, and how messages are framed on the wire.
The subsystem splits cleanly into a platform crate (`sp42-coordination`) owning
the protocol and a server shell owning the transport and relay, so the contract
is worth recording.

## Decision

### 1. Six message kinds, room-keyed by `wiki_id`
`CoordinationMessage` (`sp42-coordination::messages`): `ActionBroadcast` (a
moderation action taken), `EditClaim` (an operator claims a revision),
`ScoreDelta` (accumulating risk-score adjustment), `PresenceHeartbeat`
(presence; `active_edit_count == 0` is the leave signal), `FlaggedEdit` (absolute
flag score), and `RaceResolution` (conflict verdict). Every payload carries
`wiki_id` (the room key); most carry `rev_id` (the item).

### 2. Server-authoritative hub-and-spoke relay
All messages flow client → server → broadcast; there is no peer-to-peer path.
The `sp42-server` shell holds one `CoordinationRoomState` per `wiki_id` (a
per-room `tokio::sync::broadcast` channel, capacity 128) and the canonical
`CoordinationState`. On publish it folds the message into authoritative state
(counting accepted vs invalid), then fans out to every subscriber **except the
originator** (echo suppression by `sender_id`). New subscribers get live-only
delivery — there is no backfill; late joiners recover via the REST snapshot /
room-state / inspection routes.

### 3. Authenticated actor rewriting
The server overwrites the client-supplied identity field
(`actor`/`winning_actor`) on `ActionBroadcast`, `EditClaim`, `PresenceHeartbeat`,
and `RaceResolution` with the session username before fan-out, so a client cannot
spoof another operator. Anonymous connections (no session) pass the
client-supplied actor through unchanged.

### 4. A single deterministic reducer holds room state
`CoordinationState::apply(&mut self, CoordinationMessage) -> bool`
(`sp42-coordination::state`) is the one mutation path: it rejects any message
whose `wiki_id` mismatches the room, then folds per kind into `BTreeMap`-keyed
`claims`, `presence`, `flagged_edits`, `score_deltas`, `race_resolutions`, and a
bounded (25) `recent_actions` log. Both server and clients run the same reducer;
clients apply optimistically via `CoordinationRuntime` before the server echo.

### 5. Conflict resolution: last-writer-wins until pinned
Before any `RaceResolution` for a revision, an `EditClaim` simply overwrites the
prior claim (last-writer-wins). A `RaceResolution` pins the `winning_actor`;
thereafter claims are accepted only from the winner and competing claims are
silently dropped.

### 6. MessagePack over the `WebSocket` trait
The codec (`sp42-coordination::codec`) is MessagePack via `rmp_serde`
(`to_vec_named`, so field names are on the wire), transported as WebSocket Binary
frames over the `WebSocket` trait from `sp42-types` (the client also accepts Text
frames). The client and runtime are generic over `S: WebSocket`, so the protocol
is transport-injectable and fixture-testable.

## Consequences

- Layer split confirms ADR-0013: `sp42-coordination` is platform (message types,
  codec, reducer, client, runtime; depends only on `sp42-platform` + `sp42-types`,
  `#![forbid(unsafe_code)]`, no Axum), and `sp42-server` is the shell (WebSocket
  upgrade, `CoordinationRegistry` fan-out, actor rewriting, room lifecycle).
- The rooms are bounded and self-pruning: 128-message channel, 25 recent actions,
  60 s presence TTL, 5 min idle-room eviction; all counters saturate. The clock
  is injectable for deterministic eviction/TTL tests.
- Two presence-staleness mechanisms coexist by design: the reducer drops presence
  on a zero-count heartbeat; the server additionally time-expires presence
  (heartbeats carry no timestamp, so the server keeps a `presence_last_seen_ms`
  shadow).
- Pinned by unit tests (reducer folds and race resolution in `state.rs`; codec
  round-trip incl. a proptest over all six kinds; client/runtime), registry tests
  (`coordination.rs`: fan-out, room isolation, pruning, idle eviction with an
  injected clock), and end-to-end relay tests in `sp42-server/src/tests.rs`
  (authenticated multi-user flow, actor rewrite, invalid-payload counting,
  reconnect storms, REST recovery of race-resolved state).

## Non-goals

- The user-facing collaboration behavior and workflow — PRD-0006.
- The WebSocket transport implementation itself — the `WebSocket` trait
  (`sp42-types`); this contract is generic over it.
- Invalid payloads are counted and still fanned out verbatim (the raw bytes reach
  peers even when the server cannot decode them) — a deliberate
  forward-compatibility choice, not a validation gate.
