//! Deterministic client-side coordination state reducer.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::types::{
    ActionBroadcast, CoordinationMessage, EditClaim, FlaggedEdit, PresenceHeartbeat,
    RaceResolution, ScoreDelta,
};

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
                        current.reason = format!("{} | {}", current.reason, delta.reason);
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
    }
}

#[cfg(test)]
mod tests {
    use crate::types::{
        Action, ActionBroadcast, CoordinationMessage, EditClaim, FlaggedEdit, PresenceHeartbeat,
        RaceResolution, ScoreDelta,
    };

    use super::CoordinationState;

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
