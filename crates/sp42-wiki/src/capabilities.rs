//! Pure capability profile derivation for authenticated Wikimedia access.

use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WikiActionTokenAvailability {
    pub csrf_token_available: bool,
    pub patrol_token_available: bool,
    pub rollback_token_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WikiCapabilityProfileInput<'a> {
    pub wiki_id: &'a str,
    pub oauth_grants: &'a [String],
    pub wiki_rights: &'a [String],
    pub tokens: WikiActionTokenAvailability,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WikiCapabilityProfile {
    pub read: WikiReadCapabilityProfile,
    pub editing: WikiEditingCapabilityProfile,
    pub moderation: WikiModerationCapabilityProfile,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WikiReadCapabilityProfile {
    pub can_authenticate: bool,
    pub can_query_userinfo: bool,
    pub can_read_recent_changes: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WikiEditingCapabilityProfile {
    pub can_edit: bool,
    pub can_undo: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WikiModerationCapabilityProfile {
    pub can_patrol: bool,
    pub can_rollback: bool,
}

#[must_use]
pub fn derive_wiki_capability_profile(
    input: &WikiCapabilityProfileInput<'_>,
) -> WikiCapabilityProfile {
    let grant_set: BTreeSet<_> = input.oauth_grants.iter().map(String::as_str).collect();
    let right_set: BTreeSet<_> = input.wiki_rights.iter().map(String::as_str).collect();

    let can_edit = grant_set.contains("editpage")
        && right_set.contains("edit")
        && input.tokens.csrf_token_available;
    let can_patrol = grant_set.contains("patrol")
        && right_set.contains("patrol")
        && input.tokens.patrol_token_available;
    let can_rollback = grant_set.contains("rollback")
        && right_set.contains("rollback")
        && input.tokens.rollback_token_available;

    let mut notes = vec![
        "SP42 recentchanges reads do not require authentication; the token is needed for user-linked actions and rights validation.".to_string(),
        format!(
            "Capability profile derived from OAuth grants, wiki rights, and action tokens for {}.",
            input.wiki_id
        ),
    ];

    if grant_set.contains("rollback") && !right_set.contains("rollback") {
        notes.push(format!(
            "The token carries the OAuth rollback grant, but the account does not currently have the rollback right on {}.",
            input.wiki_id
        ));
    }

    if input.tokens.rollback_token_available && !right_set.contains("rollback") {
        notes.push(
            "A rollback token was returned by the API, but SP42 still treats rollback as unavailable because the wiki right is missing.".to_string(),
        );
    }

    if grant_set.contains("patrol") && !right_set.contains("patrol") {
        notes.push(format!(
            "The token carries the OAuth patrol grant, but the account does not currently have the patrol right on {}.",
            input.wiki_id
        ));
    }

    WikiCapabilityProfile {
        read: WikiReadCapabilityProfile {
            can_authenticate: true,
            can_query_userinfo: true,
            can_read_recent_changes: true,
        },
        editing: WikiEditingCapabilityProfile {
            can_edit,
            can_undo: can_edit,
        },
        moderation: WikiModerationCapabilityProfile {
            can_patrol,
            can_rollback,
        },
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        WikiActionTokenAvailability, WikiCapabilityProfileInput, derive_wiki_capability_profile,
    };

    #[test]
    fn derives_edit_patrol_and_rollback_when_grants_rights_and_tokens_align() {
        let grants = vec![
            "editpage".to_string(),
            "patrol".to_string(),
            "rollback".to_string(),
        ];
        let rights = vec![
            "edit".to_string(),
            "patrol".to_string(),
            "rollback".to_string(),
        ];
        let profile = derive_wiki_capability_profile(&WikiCapabilityProfileInput {
            wiki_id: "frwiki",
            oauth_grants: &grants,
            wiki_rights: &rights,
            tokens: WikiActionTokenAvailability {
                csrf_token_available: true,
                patrol_token_available: true,
                rollback_token_available: true,
            },
        });

        assert!(profile.editing.can_edit);
        assert!(profile.editing.can_undo);
        assert!(profile.moderation.can_patrol);
        assert!(profile.moderation.can_rollback);
    }

    #[test]
    fn keeps_moderation_unavailable_when_wiki_right_is_missing() {
        let grants = vec!["patrol".to_string(), "rollback".to_string()];
        let rights = Vec::new();
        let profile = derive_wiki_capability_profile(&WikiCapabilityProfileInput {
            wiki_id: "frwiki",
            oauth_grants: &grants,
            wiki_rights: &rights,
            tokens: WikiActionTokenAvailability {
                csrf_token_available: false,
                patrol_token_available: true,
                rollback_token_available: true,
            },
        });

        assert!(!profile.moderation.can_patrol);
        assert!(!profile.moderation.can_rollback);
        assert!(
            profile
                .notes
                .iter()
                .any(|note| note.contains("patrol right"))
        );
        assert!(
            profile
                .notes
                .iter()
                .any(|note| note.contains("rollback right"))
        );
    }
}
