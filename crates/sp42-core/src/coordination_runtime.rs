//! Shared runtime that couples coordination transport with deterministic state.

use crate::coordination_client::CoordinationClient;
use crate::errors::CoordinationError;
use crate::traits::WebSocket;
use crate::types::{
    Action, ActionBroadcast, CoordinationMessage, EditClaim, FlaggedEdit, PresenceHeartbeat,
    RaceResolution, ScoreDelta,
};
use crate::{CoordinationState, CoordinationStateSummary};
use tracing::warn;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoordinationRuntimeStatus {
    pub wiki_id: String,
    pub outgoing_messages: u64,
    pub incoming_messages: u64,
    pub claim_count: usize,
    pub presence_count: usize,
    pub flagged_edit_count: usize,
    pub score_delta_count: usize,
    pub race_resolution_count: usize,
    pub recent_action_count: usize,
    pub stream_closed: bool,
}

pub struct CoordinationRuntime<S> {
    client: CoordinationClient<S>,
    state: CoordinationState,
    outgoing_messages: u64,
    incoming_messages: u64,
    stream_closed: bool,
}

impl<S> CoordinationRuntime<S> {
    #[must_use]
    pub fn new(wiki_id: impl Into<String>, socket: S) -> Self {
        let wiki_id = wiki_id.into();
        Self {
            client: CoordinationClient::new(socket),
            state: CoordinationState::new(wiki_id),
            outgoing_messages: 0,
            incoming_messages: 0,
            stream_closed: false,
        }
    }

    #[must_use]
    pub fn state(&self) -> &CoordinationState {
        &self.state
    }

    #[must_use]
    pub fn summary(&self) -> CoordinationStateSummary {
        self.state.summary()
    }

    #[must_use]
    pub fn status(&self) -> CoordinationRuntimeStatus {
        CoordinationRuntimeStatus {
            wiki_id: self.state.wiki_id().to_string(),
            outgoing_messages: self.outgoing_messages,
            incoming_messages: self.incoming_messages,
            claim_count: self.state.claim_count(),
            presence_count: self.state.presence_count(),
            flagged_edit_count: self.state.flagged_edit_count(),
            score_delta_count: self.state.score_delta_count(),
            race_resolution_count: self.state.race_resolution_count(),
            recent_action_count: self.state.recent_action_count(),
            stream_closed: self.stream_closed,
        }
    }

    #[must_use]
    pub fn into_inner(self) -> S {
        self.client.into_inner()
    }
}

impl<S> CoordinationRuntime<S>
where
    S: WebSocket,
{
    /// Broadcast an edit claim and optimistically fold it into local state.
    ///
    /// # Errors
    ///
    /// Returns [`CoordinationError`] when the underlying coordination transport
    /// cannot send the claim.
    pub async fn claim_edit(
        &mut self,
        wiki_id: &str,
        rev_id: u64,
        actor: &str,
    ) -> Result<(), CoordinationError> {
        self.client.claim_edit(wiki_id, rev_id, actor).await?;
        self.outgoing_messages = self.outgoing_messages.saturating_add(1);
        let applied = self.state.apply(CoordinationMessage::EditClaim(EditClaim {
            wiki_id: wiki_id.to_string(),
            rev_id,
            actor: actor.to_string(),
        }));
        if !applied {
            warn!(
                wiki_id,
                rev_id, actor, "claim edit message was rejected by local coordination state"
            );
        }
        Ok(())
    }

    /// Broadcast an action event and optimistically fold it into local state.
    ///
    /// # Errors
    ///
    /// Returns [`CoordinationError`] when the underlying coordination transport
    /// cannot send the action.
    pub async fn broadcast_action(
        &mut self,
        wiki_id: &str,
        rev_id: u64,
        action: Action,
        actor: &str,
    ) -> Result<(), CoordinationError> {
        self.client
            .broadcast_action(wiki_id, rev_id, action.clone(), actor)
            .await?;
        self.outgoing_messages = self.outgoing_messages.saturating_add(1);
        let applied = self
            .state
            .apply(CoordinationMessage::ActionBroadcast(ActionBroadcast {
                wiki_id: wiki_id.to_string(),
                rev_id,
                action,
                actor: actor.to_string(),
            }));
        if !applied {
            warn!(
                wiki_id,
                rev_id, actor, "action broadcast was rejected by local coordination state"
            );
        }
        Ok(())
    }

    /// Broadcast a score delta and optimistically fold it into local state.
    ///
    /// # Errors
    ///
    /// Returns [`CoordinationError`] when the underlying coordination transport
    /// cannot send the delta.
    pub async fn send_score_delta(
        &mut self,
        wiki_id: &str,
        rev_id: u64,
        delta: i32,
        reason: &str,
    ) -> Result<(), CoordinationError> {
        self.client
            .send_score_delta(wiki_id, rev_id, delta, reason)
            .await?;
        self.outgoing_messages = self.outgoing_messages.saturating_add(1);
        let applied = self
            .state
            .apply(CoordinationMessage::ScoreDelta(ScoreDelta {
                wiki_id: wiki_id.to_string(),
                rev_id,
                delta,
                reason: reason.to_string(),
            }));
        if !applied {
            warn!(
                wiki_id,
                rev_id, delta, reason, "score delta was rejected by local coordination state"
            );
        }
        Ok(())
    }

    /// Broadcast a presence heartbeat and optimistically fold it into local
    /// state.
    ///
    /// # Errors
    ///
    /// Returns [`CoordinationError`] when the underlying coordination transport
    /// cannot send the heartbeat.
    pub async fn send_presence(
        &mut self,
        wiki_id: &str,
        actor: &str,
        active_edit_count: u32,
    ) -> Result<(), CoordinationError> {
        self.client
            .send_presence(wiki_id, actor, active_edit_count)
            .await?;
        self.outgoing_messages = self.outgoing_messages.saturating_add(1);
        let applied = self
            .state
            .apply(CoordinationMessage::PresenceHeartbeat(PresenceHeartbeat {
                wiki_id: wiki_id.to_string(),
                actor: actor.to_string(),
                active_edit_count,
            }));
        if !applied {
            warn!(
                wiki_id,
                actor,
                active_edit_count,
                "presence heartbeat was rejected by local coordination state"
            );
        }
        Ok(())
    }

    /// Broadcast a flagged edit and optimistically fold it into local state.
    ///
    /// # Errors
    ///
    /// Returns [`CoordinationError`] when the underlying coordination transport
    /// cannot send the flag.
    pub async fn flag_edit(
        &mut self,
        wiki_id: &str,
        rev_id: u64,
        score: i32,
        reason: &str,
    ) -> Result<(), CoordinationError> {
        self.client
            .flag_edit(wiki_id, rev_id, score, reason)
            .await?;
        self.outgoing_messages = self.outgoing_messages.saturating_add(1);
        let applied = self
            .state
            .apply(CoordinationMessage::FlaggedEdit(FlaggedEdit {
                wiki_id: wiki_id.to_string(),
                rev_id,
                score,
                reason: reason.to_string(),
            }));
        if !applied {
            warn!(
                wiki_id,
                rev_id, score, reason, "flagged edit was rejected by local coordination state"
            );
        }
        Ok(())
    }

    /// Broadcast a race resolution and optimistically fold it into local state.
    ///
    /// # Errors
    ///
    /// Returns [`CoordinationError`] when the underlying coordination transport
    /// cannot send the resolution.
    pub async fn resolve_race(
        &mut self,
        wiki_id: &str,
        rev_id: u64,
        winning_actor: &str,
    ) -> Result<(), CoordinationError> {
        self.client
            .send_race_resolution(wiki_id, rev_id, winning_actor)
            .await?;
        self.outgoing_messages = self.outgoing_messages.saturating_add(1);
        let applied = self
            .state
            .apply(CoordinationMessage::RaceResolution(RaceResolution {
                wiki_id: wiki_id.to_string(),
                rev_id,
                winning_actor: winning_actor.to_string(),
            }));
        if !applied {
            warn!(
                wiki_id,
                rev_id, winning_actor, "race resolution was rejected by local coordination state"
            );
        }
        Ok(())
    }

    /// Receive one coordination message, fold it into local state, and return
    /// the decoded payload.
    ///
    /// # Errors
    ///
    /// Returns [`CoordinationError`] when the underlying coordination transport
    /// or message decoding fails.
    pub async fn receive_message(
        &mut self,
    ) -> Result<Option<CoordinationMessage>, CoordinationError> {
        if let Some(message) = self.client.receive_message().await? {
            self.incoming_messages = self.incoming_messages.saturating_add(1);
            if !self.state.apply(message.clone()) {
                warn!(message = ?message, "incoming coordination message was rejected by local coordination state");
            }
            Ok(Some(message))
        } else {
            self.stream_closed = true;
            Ok(None)
        }
    }

    /// Drain up to `limit` decoded coordination messages from the transport and
    /// fold them into local state.
    ///
    /// # Errors
    ///
    /// Returns [`CoordinationError`] when the underlying coordination transport
    /// or message decoding fails.
    pub async fn drain_incoming(
        &mut self,
        limit: usize,
    ) -> Result<Vec<CoordinationMessage>, CoordinationError> {
        let mut messages = Vec::new();
        while messages.len() < limit {
            match self.receive_message().await? {
                Some(message) => messages.push(message),
                None => break,
            }
        }
        Ok(messages)
    }
}

#[cfg(test)]
mod tests {
    use futures::executor::block_on;

    use super::CoordinationRuntime;
    use crate::coordination_codec::encode_message;
    use crate::traits::LoopbackWebSocket;
    use crate::types::{Action, CoordinationMessage, EditClaim, WebSocketFrame};

    #[test]
    fn optimistic_claim_updates_state_and_status() {
        let socket = LoopbackWebSocket::default();
        let mut runtime = CoordinationRuntime::new("frwiki", socket);

        block_on(runtime.claim_edit("frwiki", 42, "Alice")).expect("claim should succeed");

        assert_eq!(
            runtime
                .state()
                .claim_for(42)
                .map(|claim| claim.actor.as_str()),
            Some("Alice")
        );
        assert_eq!(runtime.status().outgoing_messages, 1);
    }

    #[test]
    fn receive_message_updates_state() {
        let payload = encode_message(&CoordinationMessage::EditClaim(EditClaim {
            wiki_id: "frwiki".to_string(),
            rev_id: 77,
            actor: "Bob".to_string(),
        }))
        .expect("payload should encode");
        let socket = LoopbackWebSocket::with_incoming([WebSocketFrame::Binary(payload)]);
        let mut runtime = CoordinationRuntime::new("frwiki", socket);

        let message = block_on(runtime.receive_message())
            .expect("receive should succeed")
            .expect("message should exist");

        assert!(matches!(message, CoordinationMessage::EditClaim(_)));
        assert_eq!(
            runtime
                .state()
                .claim_for(77)
                .map(|claim| claim.actor.as_str()),
            Some("Bob")
        );
        assert_eq!(runtime.status().incoming_messages, 1);
    }

    #[test]
    fn drain_incoming_stops_at_limit() {
        let payload_a = encode_message(&CoordinationMessage::ActionBroadcast(
            crate::types::ActionBroadcast {
                wiki_id: "frwiki".to_string(),
                rev_id: 11,
                action: Action::Rollback,
                actor: "Alice".to_string(),
            },
        ))
        .expect("payload should encode");
        let payload_b = encode_message(&CoordinationMessage::EditClaim(EditClaim {
            wiki_id: "frwiki".to_string(),
            rev_id: 12,
            actor: "Bob".to_string(),
        }))
        .expect("payload should encode");
        let socket = LoopbackWebSocket::with_incoming([
            WebSocketFrame::Binary(payload_a),
            WebSocketFrame::Binary(payload_b),
        ]);
        let mut runtime = CoordinationRuntime::new("frwiki", socket);

        let messages = block_on(runtime.drain_incoming(1)).expect("drain should succeed");

        assert_eq!(messages.len(), 1);
        assert_eq!(runtime.status().incoming_messages, 1);
    }
}
