//! Coordination protocol client helpers built on the injected `WebSocket` trait.

use crate::coordination_codec::{decode_message, encode_message};
use crate::errors::CoordinationError;
use crate::traits::WebSocket;
use crate::types::{
    Action, ActionBroadcast, CoordinationMessage, EditClaim, FlaggedEdit, PresenceHeartbeat,
    RaceResolution, ScoreDelta, WebSocketFrame,
};

pub struct CoordinationClient<S> {
    socket: S,
}

impl<S> CoordinationClient<S> {
    #[must_use]
    pub fn new(socket: S) -> Self {
        Self { socket }
    }

    #[must_use]
    pub fn into_inner(self) -> S {
        self.socket
    }
}

impl<S> CoordinationClient<S>
where
    S: WebSocket,
{
    /// Broadcast an edit claim to the coordination room.
    ///
    /// # Errors
    ///
    /// Returns [`CoordinationError`] when encoding or transport fails.
    pub async fn claim_edit(
        &mut self,
        wiki_id: &str,
        rev_id: u64,
        actor: &str,
    ) -> Result<(), CoordinationError> {
        self.send_message(CoordinationMessage::EditClaim(EditClaim {
            wiki_id: wiki_id.to_string(),
            rev_id,
            actor: actor.to_string(),
        }))
        .await
    }

    /// Broadcast an action event to the coordination room.
    ///
    /// # Errors
    ///
    /// Returns [`CoordinationError`] when encoding or transport fails.
    pub async fn broadcast_action(
        &mut self,
        wiki_id: &str,
        rev_id: u64,
        action: Action,
        actor: &str,
    ) -> Result<(), CoordinationError> {
        self.send_message(CoordinationMessage::ActionBroadcast(ActionBroadcast {
            wiki_id: wiki_id.to_string(),
            rev_id,
            action,
            actor: actor.to_string(),
        }))
        .await
    }

    /// Broadcast a score delta event.
    ///
    /// # Errors
    ///
    /// Returns [`CoordinationError`] when encoding or transport fails.
    pub async fn send_score_delta(
        &mut self,
        wiki_id: &str,
        rev_id: u64,
        delta: i32,
        reason: &str,
    ) -> Result<(), CoordinationError> {
        self.send_message(CoordinationMessage::ScoreDelta(ScoreDelta {
            wiki_id: wiki_id.to_string(),
            rev_id,
            delta,
            reason: reason.to_string(),
        }))
        .await
    }

    /// Broadcast a presence heartbeat.
    ///
    /// # Errors
    ///
    /// Returns [`CoordinationError`] when encoding or transport fails.
    pub async fn send_presence(
        &mut self,
        wiki_id: &str,
        actor: &str,
        active_edit_count: u32,
    ) -> Result<(), CoordinationError> {
        self.send_message(CoordinationMessage::PresenceHeartbeat(PresenceHeartbeat {
            wiki_id: wiki_id.to_string(),
            actor: actor.to_string(),
            active_edit_count,
        }))
        .await
    }

    /// Broadcast a flagged edit message.
    ///
    /// # Errors
    ///
    /// Returns [`CoordinationError`] when encoding or transport fails.
    pub async fn flag_edit(
        &mut self,
        wiki_id: &str,
        rev_id: u64,
        score: i32,
        reason: &str,
    ) -> Result<(), CoordinationError> {
        self.send_message(CoordinationMessage::FlaggedEdit(FlaggedEdit {
            wiki_id: wiki_id.to_string(),
            rev_id,
            score,
            reason: reason.to_string(),
        }))
        .await
    }

    /// Broadcast a race resolution event.
    ///
    /// # Errors
    ///
    /// Returns [`CoordinationError`] when encoding or transport fails.
    pub async fn send_race_resolution(
        &mut self,
        wiki_id: &str,
        rev_id: u64,
        winning_actor: &str,
    ) -> Result<(), CoordinationError> {
        self.send_message(CoordinationMessage::RaceResolution(RaceResolution {
            wiki_id: wiki_id.to_string(),
            rev_id,
            winning_actor: winning_actor.to_string(),
        }))
        .await
    }

    /// Receive and decode the next coordination message from the websocket.
    ///
    /// # Errors
    ///
    /// Returns [`CoordinationError`] when the socket frame is invalid or the
    /// payload cannot be decoded.
    pub async fn receive_message(
        &mut self,
    ) -> Result<Option<CoordinationMessage>, CoordinationError> {
        let Some(frame) = self.socket.receive().await? else {
            return Ok(None);
        };

        match frame {
            WebSocketFrame::Binary(bytes) => decode_message(&bytes)
                .map(Some)
                .map_err(CoordinationError::from),
            WebSocketFrame::Text(text) => decode_message(text.as_bytes())
                .map(Some)
                .map_err(CoordinationError::from),
            WebSocketFrame::Close => Ok(None),
        }
    }

    async fn send_message(
        &mut self,
        message: CoordinationMessage,
    ) -> Result<(), CoordinationError> {
        let payload = encode_message(&message)?;
        self.socket.send(WebSocketFrame::Binary(payload)).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use futures::executor::block_on;

    use super::CoordinationClient;
    use crate::coordination_codec::encode_message;
    use crate::traits::LoopbackWebSocket;
    use crate::types::{
        Action, CoordinationMessage, EditClaim, PresenceHeartbeat, RaceResolution, WebSocketFrame,
    };

    #[test]
    fn sends_edit_claim_frame() {
        let socket = LoopbackWebSocket::default();
        let mut client = CoordinationClient::new(socket);

        block_on(client.claim_edit("frwiki", 42, "Example")).expect("claim should succeed");
        let socket = client.into_inner();

        assert_eq!(socket.sent_frames().len(), 1);
        assert!(matches!(socket.sent_frames()[0], WebSocketFrame::Binary(_)));
    }

    #[test]
    fn decodes_incoming_message() {
        let payload = encode_message(&CoordinationMessage::EditClaim(EditClaim {
            wiki_id: "frwiki".to_string(),
            rev_id: 42,
            actor: "Example".to_string(),
        }))
        .expect("payload should encode");
        let socket = LoopbackWebSocket::with_incoming([WebSocketFrame::Binary(payload)]);
        let mut client = CoordinationClient::new(socket);

        let message = block_on(client.receive_message())
            .expect("receive should succeed")
            .expect("message should exist");

        assert_eq!(
            message,
            CoordinationMessage::EditClaim(EditClaim {
                wiki_id: "frwiki".to_string(),
                rev_id: 42,
                actor: "Example".to_string(),
            })
        );
    }

    #[test]
    fn sends_presence_heartbeat_frame() {
        let socket = LoopbackWebSocket::default();
        let mut client = CoordinationClient::new(socket);

        block_on(client.send_presence("frwiki", "Example", 3)).expect("presence should succeed");
        let socket = client.into_inner();
        let payload = match &socket.sent_frames()[0] {
            WebSocketFrame::Binary(bytes) => bytes.clone(),
            frame => panic!("unexpected frame: {frame:?}"),
        };
        let decoded =
            crate::coordination_codec::decode_message(&payload).expect("payload should decode");

        assert_eq!(
            decoded,
            CoordinationMessage::PresenceHeartbeat(PresenceHeartbeat {
                wiki_id: "frwiki".to_string(),
                actor: "Example".to_string(),
                active_edit_count: 3,
            })
        );
    }

    #[test]
    fn broadcasts_action_frame() {
        let socket = LoopbackWebSocket::default();
        let mut client = CoordinationClient::new(socket);

        block_on(client.broadcast_action("frwiki", 99, Action::Rollback, "Reviewer"))
            .expect("broadcast should succeed");
        let socket = client.into_inner();

        assert_eq!(socket.sent_frames().len(), 1);
    }

    #[test]
    fn sends_race_resolution_frame() {
        let socket = LoopbackWebSocket::default();
        let mut client = CoordinationClient::new(socket);

        block_on(client.send_race_resolution("frwiki", 88, "Reviewer"))
            .expect("resolution should succeed");
        let socket = client.into_inner();
        let payload = match &socket.sent_frames()[0] {
            WebSocketFrame::Binary(bytes) => bytes.clone(),
            frame => panic!("unexpected frame: {frame:?}"),
        };
        let decoded =
            crate::coordination_codec::decode_message(&payload).expect("payload should decode");

        assert_eq!(
            decoded,
            CoordinationMessage::RaceResolution(RaceResolution {
                wiki_id: "frwiki".to_string(),
                rev_id: 88,
                winning_actor: "Reviewer".to_string(),
            })
        );
    }
}
