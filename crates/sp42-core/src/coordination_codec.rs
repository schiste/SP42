//! `MessagePack` codec helpers for coordination messages.

use crate::errors::CodecError;
use crate::types::CoordinationMessage;

/// Encode a coordination message to `MessagePack`.
///
/// # Errors
///
/// Returns [`CodecError`] when the message cannot be serialized.
pub fn encode_message(message: &CoordinationMessage) -> Result<Vec<u8>, CodecError> {
    rmp_serde::to_vec_named(message).map_err(CodecError::from)
}

/// Decode a coordination message from `MessagePack`.
///
/// # Errors
///
/// Returns [`CodecError`] when the byte payload is not a valid coordination
/// message.
pub fn decode_message(bytes: &[u8]) -> Result<CoordinationMessage, CodecError> {
    rmp_serde::from_slice(bytes).map_err(CodecError::from)
}

#[cfg(test)]
mod tests {
    use super::{decode_message, encode_message};
    use crate::types::{
        Action, ActionBroadcast, CoordinationMessage, EditClaim, FlaggedEdit, PresenceHeartbeat,
        RaceResolution, ScoreDelta,
    };
    use proptest::prelude::*;

    #[test]
    fn round_trip_identity() {
        let message = CoordinationMessage::EditClaim(EditClaim {
            wiki_id: "frwiki".to_string(),
            rev_id: 42,
            actor: "Example".to_string(),
        });

        let bytes = encode_message(&message).expect("encoding should succeed");
        let decoded = decode_message(&bytes).expect("decoding should succeed");

        assert_eq!(decoded, message);
    }

    fn action_strategy() -> impl Strategy<Value = Action> {
        prop_oneof![
            Just(Action::Rollback),
            Just(Action::Revert),
            Just(Action::Warn),
            Just(Action::Report),
            Just(Action::MarkPatrolled),
        ]
    }

    fn text_strategy() -> impl Strategy<Value = String> {
        "[a-zA-Z0-9 _-]{1,24}".prop_map(|value| value)
    }

    fn message_strategy() -> impl Strategy<Value = CoordinationMessage> {
        prop_oneof![
            (
                text_strategy(),
                1_u64..10_000,
                action_strategy(),
                text_strategy()
            )
                .prop_map(|(wiki_id, rev_id, action, actor)| {
                    CoordinationMessage::ActionBroadcast(ActionBroadcast {
                        wiki_id,
                        rev_id,
                        action,
                        actor,
                    })
                }),
            (text_strategy(), 1_u64..10_000, text_strategy()).prop_map(
                |(wiki_id, rev_id, actor)| CoordinationMessage::EditClaim(EditClaim {
                    wiki_id,
                    rev_id,
                    actor,
                })
            ),
            (
                text_strategy(),
                1_u64..10_000,
                -200i32..200,
                text_strategy()
            )
                .prop_map(|(wiki_id, rev_id, delta, reason)| {
                    CoordinationMessage::ScoreDelta(ScoreDelta {
                        wiki_id,
                        rev_id,
                        delta,
                        reason,
                    })
                }),
            (text_strategy(), text_strategy(), 0_u32..20).prop_map(
                |(wiki_id, actor, active_edit_count)| CoordinationMessage::PresenceHeartbeat(
                    PresenceHeartbeat {
                        wiki_id,
                        actor,
                        active_edit_count,
                    }
                )
            ),
            (text_strategy(), 1_u64..10_000, 0i32..100, text_strategy()).prop_map(
                |(wiki_id, rev_id, score, reason)| CoordinationMessage::FlaggedEdit(FlaggedEdit {
                    wiki_id,
                    rev_id,
                    score,
                    reason,
                })
            ),
            (text_strategy(), 1_u64..10_000, text_strategy()).prop_map(
                |(wiki_id, rev_id, winning_actor)| CoordinationMessage::RaceResolution(
                    RaceResolution {
                        wiki_id,
                        rev_id,
                        winning_actor,
                    }
                )
            ),
        ]
    }

    proptest! {
        #[test]
        fn property_round_trip_identity(message in message_strategy()) {
            let bytes = encode_message(&message).expect("encoding should succeed");
            let decoded = decode_message(&bytes).expect("decoding should succeed");

            prop_assert_eq!(decoded, message);
        }
    }
}
