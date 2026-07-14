//! Deterministic client-side coordination state reducer.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::messages::{
    ActionBroadcast, CoordinationMessage, EditClaim, FlaggedEdit, PresenceHeartbeat,
    RaceResolution, ScoreDelta,
};

/// Upper bound on the length (in bytes) of an accumulated `ScoreDelta` reason.
///
/// Each incoming delta for a given `rev_id` appends its reason onto the stored
/// one, so an unbounded stream of deltas — from a chatty, buggy, or hostile
/// client — would grow this `String` without limit, and it is cloned into every
/// state snapshot. Capping keeps the most recent reason text and drops the
/// oldest once the bound is reached.
const MAX_ACCUMULATED_REASON_LEN: usize = 4096;

/// Append `next` onto `current` (separated by `" | "`), keeping the total under
/// [`MAX_ACCUMULATED_REASON_LEN`] bytes by discarding the oldest leading text on
/// a UTF-8 character boundary.
fn append_capped_reason(current: &mut String, next: &str) {
    if !current.is_empty() {
        current.push_str(" | ");
    }
    current.push_str(next);
    if current.len() <= MAX_ACCUMULATED_REASON_LEN {
        return;
    }
    // Drop from the front so the freshest reason survives, then advance to the
    // next char boundary so we never split a multi-byte sequence.
    let mut cut = current.len() - MAX_ACCUMULATED_REASON_LEN;
    while cut < current.len() && !current.is_char_boundary(cut) {
        cut += 1;
    }
    current.drain(..cut);
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoordinationStateSummary {
    pub wiki_id: String,
    pub claims: Vec<EditClaim>,
    pub presence: Vec<PresenceHeartbeat>,
    pub flagged_edits: Vec<FlaggedEdit>,
    pub score_deltas: Vec<ScoreDelta>,
    pub race_resolutions: Vec<RaceResolution>,
    pub recent_actions: Vec<ActionBroadcast>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoordinationState {
    wiki_id: String,
    claims: BTreeMap<u64, EditClaim>,
    presence: BTreeMap<String, PresenceHeartbeat>,
    flagged_edits: BTreeMap<u64, FlaggedEdit>,
    score_deltas: BTreeMap<u64, ScoreDelta>,
    race_resolutions: BTreeMap<u64, RaceResolution>,
    recent_actions: Vec<ActionBroadcast>,
}

impl CoordinationState {
    #[must_use]
    pub fn new(wiki_id: impl Into<String>) -> Self {
        Self {
            wiki_id: wiki_id.into(),
            claims: BTreeMap::new(),
            presence: BTreeMap::new(),
            flagged_edits: BTreeMap::new(),
            score_deltas: BTreeMap::new(),
            race_resolutions: BTreeMap::new(),
            recent_actions: Vec::new(),
        }
    }

    #[must_use]
    pub fn wiki_id(&self) -> &str {
        &self.wiki_id
    }

    #[must_use]
    pub fn claim_for(&self, rev_id: u64) -> Option<&EditClaim> {
        self.claims.get(&rev_id)
    }

    #[must_use]
    pub fn presence_for(&self, actor: &str) -> Option<&PresenceHeartbeat> {
        self.presence.get(actor)
    }

    #[must_use]
    pub fn presence_actors(&self) -> Vec<String> {
        self.presence.keys().cloned().collect()
    }

    pub fn remove_presence(&mut self, actor: &str) -> bool {
        self.presence.remove(actor).is_some()
    }

    #[must_use]
    pub fn flagged_edit(&self, rev_id: u64) -> Option<&FlaggedEdit> {
        self.flagged_edits.get(&rev_id)
    }

    #[must_use]
    pub fn score_delta(&self, rev_id: u64) -> Option<&ScoreDelta> {
        self.score_deltas.get(&rev_id)
    }

    #[must_use]
    pub fn race_resolution(&self, rev_id: u64) -> Option<&RaceResolution> {
        self.race_resolutions.get(&rev_id)
    }

    pub fn apply(&mut self, message: CoordinationMessage) -> bool {
        if message_wiki_id(&message) != self.wiki_id {
            return false;
        }

        match message {
            CoordinationMessage::EditClaim(claim) => {
                if self
                    .race_resolutions
                    .get(&claim.rev_id)
                    .is_none_or(|resolution| resolution.winning_actor == claim.actor)
                {
                    self.claims.insert(claim.rev_id, claim);
                }
            }
            CoordinationMessage::PresenceHeartbeat(heartbeat) => {
                if heartbeat.active_edit_count == 0 {
                    self.presence.remove(&heartbeat.actor);
                } else {
                    self.presence.insert(heartbeat.actor.clone(), heartbeat);
                }
            }
            CoordinationMessage::FlaggedEdit(flagged) => {
                self.flagged_edits.insert(flagged.rev_id, flagged);
            }
            CoordinationMessage::ScoreDelta(delta) => {
                self.score_deltas
                    .entry(delta.rev_id)
                    .and_modify(|current| {
                        current.delta = current.delta.saturating_add(delta.delta);
                        append_capped_reason(&mut current.reason, &delta.reason);
                    })
                    .or_insert(delta);
            }
            CoordinationMessage::RaceResolution(resolution) => {
                self.claims.insert(
                    resolution.rev_id,
                    EditClaim {
                        wiki_id: resolution.wiki_id.clone(),
                        rev_id: resolution.rev_id,
                        actor: resolution.winning_actor.clone(),
                    },
                );
                self.race_resolutions.insert(resolution.rev_id, resolution);
            }
            CoordinationMessage::ActionBroadcast(action) => {
                self.recent_actions.push(action);
                if self.recent_actions.len() > 25 {
                    let overflow = self.recent_actions.len() - 25;
                    self.recent_actions.drain(0..overflow);
                }
            }
            CoordinationMessage::ReviewSignal(signal) => {
                // Deliberately stateless: the signal is a live re-fetch hint
                // for panels, and the review-session store behind the gated
                // review routes stays the source of truth (ADR-0018 §8).
                // Folding it here would create a second, spoofable copy.
                drop(signal);
            }
        }

        true
    }

    pub fn apply_many<I>(&mut self, messages: I) -> usize
    where
        I: IntoIterator<Item = CoordinationMessage>,
    {
        messages.into_iter().fold(0usize, |count, message| {
            count + usize::from(self.apply(message))
        })
    }

    #[must_use]
    pub fn claim_count(&self) -> usize {
        self.claims.len()
    }

    #[must_use]
    pub fn presence_count(&self) -> usize {
        self.presence.len()
    }

    #[must_use]
    pub fn flagged_edit_count(&self) -> usize {
        self.flagged_edits.len()
    }

    #[must_use]
    pub fn score_delta_count(&self) -> usize {
        self.score_deltas.len()
    }

    #[must_use]
    pub fn race_resolution_count(&self) -> usize {
        self.race_resolutions.len()
    }

    #[must_use]
    pub fn recent_action_count(&self) -> usize {
        self.recent_actions.len()
    }

    #[must_use]
    pub fn summary(&self) -> CoordinationStateSummary {
        CoordinationStateSummary {
            wiki_id: self.wiki_id.clone(),
            claims: self.claims.values().cloned().collect(),
            presence: self.presence.values().cloned().collect(),
            flagged_edits: self.flagged_edits.values().cloned().collect(),
            score_deltas: self.score_deltas.values().cloned().collect(),
            race_resolutions: self.race_resolutions.values().cloned().collect(),
            recent_actions: self.recent_actions.clone(),
        }
    }
}

fn message_wiki_id(message: &CoordinationMessage) -> &str {
    match message {
        CoordinationMessage::ActionBroadcast(action) => &action.wiki_id,
        CoordinationMessage::EditClaim(claim) => &claim.wiki_id,
        CoordinationMessage::ScoreDelta(delta) => &delta.wiki_id,
        CoordinationMessage::PresenceHeartbeat(heartbeat) => &heartbeat.wiki_id,
        CoordinationMessage::FlaggedEdit(flagged) => &flagged.wiki_id,
        CoordinationMessage::RaceResolution(resolution) => &resolution.wiki_id,
        CoordinationMessage::ReviewSignal(signal) => &signal.wiki_id,
    }
}

#[cfg(test)]
mod tests {
    use crate::messages::{
        ActionBroadcast, CoordinationMessage, EditClaim, FlaggedEdit, PresenceHeartbeat,
        RaceResolution, ReviewSignal, ScoreDelta,
    };
    use sp42_platform::Action;

    use super::CoordinationState;

    fn review_signal(wiki_id: &str) -> ReviewSignal {
        ReviewSignal {
            wiki_id: wiki_id.to_string(),
            session: sp42_platform::ReviewSession::open(wiki_id, "Exemple", 42, 1_000).snapshot(),
        }
    }

    #[test]
    fn review_signal_relays_without_folding_room_state() {
        let mut state = CoordinationState::new("frwiki");
        let before = state.summary();

        // Same wiki: accepted (so the relay fans it out to panels)...
        assert!(state.apply(CoordinationMessage::ReviewSignal(review_signal("frwiki"))));
        // ...but deliberately stateless — the review-session store behind
        // the gated review routes is the source of truth (ADR-0018 §8).
        assert_eq!(state.summary(), before);

        // Other wiki: rejected like every other kind.
        assert!(!state.apply(CoordinationMessage::ReviewSignal(review_signal("enwiki"))));
    }

    #[test]
    fn ignores_messages_for_other_wikis() {
        let mut state = CoordinationState::new("frwiki");

        let applied = state.apply(CoordinationMessage::EditClaim(EditClaim {
            wiki_id: "enwiki".to_string(),
            rev_id: 1,
            actor: "Other".to_string(),
        }));

        assert!(!applied);
        assert!(state.claim_for(1).is_none());
    }

    #[test]
    fn tracks_claims_presence_flags_and_actions() {
        let mut state = CoordinationState::new("frwiki");

        assert!(state.apply(CoordinationMessage::EditClaim(EditClaim {
            wiki_id: "frwiki".to_string(),
            rev_id: 123,
            actor: "Alice".to_string(),
        })));
        assert!(
            state.apply(CoordinationMessage::PresenceHeartbeat(PresenceHeartbeat {
                wiki_id: "frwiki".to_string(),
                actor: "Alice".to_string(),
                active_edit_count: 2,
            },))
        );
        assert!(state.apply(CoordinationMessage::FlaggedEdit(FlaggedEdit {
            wiki_id: "frwiki".to_string(),
            rev_id: 123,
            score: 95,
            reason: "possible vandalism".to_string(),
        })));
        assert!(
            state.apply(CoordinationMessage::ActionBroadcast(ActionBroadcast {
                wiki_id: "frwiki".to_string(),
                rev_id: 123,
                action: Action::Rollback,
                actor: "Alice".to_string(),
            },))
        );

        assert_eq!(
            state.claim_for(123).map(|claim| claim.actor.as_str()),
            Some("Alice")
        );
        assert_eq!(
            state
                .presence_for("Alice")
                .map(|entry| entry.active_edit_count),
            Some(2)
        );
        assert_eq!(state.flagged_edit(123).map(|entry| entry.score), Some(95));
        assert_eq!(state.summary().recent_actions.len(), 1);
    }

    #[test]
    fn aggregates_score_deltas_and_applies_race_resolution() {
        let mut state = CoordinationState::new("frwiki");

        assert!(state.apply(CoordinationMessage::EditClaim(EditClaim {
            wiki_id: "frwiki".to_string(),
            rev_id: 456,
            actor: "Alice".to_string(),
        })));
        assert!(state.apply(CoordinationMessage::ScoreDelta(ScoreDelta {
            wiki_id: "frwiki".to_string(),
            rev_id: 456,
            delta: 5,
            reason: "warning history".to_string(),
        })));
        assert!(state.apply(CoordinationMessage::ScoreDelta(ScoreDelta {
            wiki_id: "frwiki".to_string(),
            rev_id: 456,
            delta: 3,
            reason: "liftwing".to_string(),
        })));
        assert!(
            state.apply(CoordinationMessage::RaceResolution(RaceResolution {
                wiki_id: "frwiki".to_string(),
                rev_id: 456,
                winning_actor: "Bob".to_string(),
            }))
        );
        assert!(state.apply(CoordinationMessage::EditClaim(EditClaim {
            wiki_id: "frwiki".to_string(),
            rev_id: 456,
            actor: "Charlie".to_string(),
        })));

        let delta = state.score_delta(456).expect("delta should exist");
        assert_eq!(delta.delta, 8);
        assert!(delta.reason.contains("warning history"));
        assert!(delta.reason.contains("liftwing"));
        assert_eq!(
            state.claim_for(456).map(|claim| claim.actor.as_str()),
            Some("Bob")
        );
    }

    #[test]
    fn accumulated_score_delta_reason_stays_bounded() {
        let mut state = CoordinationState::new("frwiki");

        // A client streams many deltas for the same rev_id; the stored reason
        // must not grow without limit.
        for i in 0..2_000 {
            assert!(state.apply(CoordinationMessage::ScoreDelta(ScoreDelta {
                wiki_id: "frwiki".to_string(),
                rev_id: 456,
                delta: 0,
                reason: format!("reason-{i}-with-some-padding-text"),
            })));
        }

        let delta = state.score_delta(456).expect("delta should exist");
        assert!(
            delta.reason.len() <= super::MAX_ACCUMULATED_REASON_LEN,
            "reason grew to {} bytes",
            delta.reason.len()
        );
        // The freshest reason survives; the oldest is dropped.
        assert!(delta.reason.contains("reason-1999-"));
        assert!(!delta.reason.contains("reason-0-"));
    }

    #[test]
    fn removes_presence_when_active_count_hits_zero() {
        let mut state = CoordinationState::new("frwiki");

        assert!(
            state.apply(CoordinationMessage::PresenceHeartbeat(PresenceHeartbeat {
                wiki_id: "frwiki".to_string(),
                actor: "Alice".to_string(),
                active_edit_count: 1,
            },))
        );
        assert!(
            state.apply(CoordinationMessage::PresenceHeartbeat(PresenceHeartbeat {
                wiki_id: "frwiki".to_string(),
                actor: "Alice".to_string(),
                active_edit_count: 0,
            },))
        );

        assert!(state.presence_for("Alice").is_none());
    }

    #[test]
    fn apply_many_returns_number_of_applied_messages() {
        let mut state = CoordinationState::new("frwiki");

        let applied = state.apply_many([
            CoordinationMessage::EditClaim(EditClaim {
                wiki_id: "frwiki".to_string(),
                rev_id: 10,
                actor: "Alice".to_string(),
            }),
            CoordinationMessage::EditClaim(EditClaim {
                wiki_id: "enwiki".to_string(),
                rev_id: 11,
                actor: "Bob".to_string(),
            }),
        ]);

        assert_eq!(applied, 1);
        assert_eq!(state.claim_count(), 1);
    }
}
