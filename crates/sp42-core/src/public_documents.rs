//! Typed public SP42 documents stored on canonical wiki pages.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::errors::PublicDocumentError;
use crate::wiki_storage::WikiStorageDocumentKind;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicUserPreferencesDocument {
    pub preferred_wiki_id: String,
    pub queue_limit: u16,
    #[serde(default)]
    pub hide_minor: bool,
    #[serde(default)]
    pub hide_bots: bool,
    #[serde(default)]
    pub editor_types: Vec<String>,
    #[serde(default)]
    pub tag_filters: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicTeamRegistryEntry {
    pub slug: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicTeamRegistryDocument {
    pub wiki_id: String,
    #[serde(default)]
    pub teams: Vec<PublicTeamRegistryEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicTeamDefinitionDocument {
    pub wiki_id: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    #[serde(default)]
    pub members: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicRuleSetDocument {
    pub wiki_id: String,
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub namespace_allowlist: Vec<i32>,
    #[serde(default)]
    pub tag_filters: Vec<String>,
    #[serde(default)]
    pub hide_minor: bool,
    #[serde(default)]
    pub hide_bots: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicAuditLedgerEntry {
    pub timestamp_ms: i64,
    pub actor: String,
    pub action: String,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicAuditLedgerDocument {
    pub wiki_id: String,
    pub period_slug: String,
    #[serde(default)]
    pub entries: Vec<PublicAuditLedgerEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "document", rename_all = "snake_case")]
pub enum PublicStorageDocumentData {
    Preferences(PublicUserPreferencesDocument),
    Registry(PublicTeamRegistryDocument),
    Team(PublicTeamDefinitionDocument),
    RuleSet(PublicRuleSetDocument),
    AuditLedger(PublicAuditLedgerDocument),
}

impl PublicStorageDocumentData {
    #[must_use]
    pub fn kind_label(&self) -> &'static str {
        match self {
            Self::Preferences(_) => "preferences",
            Self::Registry(_) => "registry",
            Self::Team(_) => "team",
            Self::RuleSet(_) => "rule_set",
            Self::AuditLedger(_) => "audit_ledger",
        }
    }

    /// # Errors
    ///
    /// Returns [`PublicDocumentError`] when the document payload does not match
    /// the expected on-wiki document kind or fails validation.
    pub fn ensure_matches_document_kind(
        &self,
        kind: &WikiStorageDocumentKind,
    ) -> Result<(), PublicDocumentError> {
        let compatible = matches!(
            (self, kind),
            (
                Self::Preferences(_),
                WikiStorageDocumentKind::PersonalPreferences
            ) | (
                Self::Registry(_),
                WikiStorageDocumentKind::SharedRegistry { .. }
            ) | (Self::Team(_), WikiStorageDocumentKind::SharedTeam { .. })
                | (
                    Self::RuleSet(_),
                    WikiStorageDocumentKind::SharedRuleSet { .. }
                )
                | (
                    Self::AuditLedger(_),
                    WikiStorageDocumentKind::SharedAuditPeriod { .. }
                )
        );
        if !compatible {
            return Err(PublicDocumentError::InvalidDocumentKind {
                message: format!(
                    "public document payload `{}` does not match storage kind `{kind:?}`",
                    self.kind_label()
                ),
            });
        }
        validate_public_storage_document(self)
    }

    /// # Errors
    ///
    /// Returns [`PublicDocumentError`] when serialization fails or validation
    /// rejects the document.
    pub fn into_json_value(self) -> Result<Value, PublicDocumentError> {
        validate_public_storage_document(&self)?;
        serde_json::to_value(self).map_err(|error| PublicDocumentError::Serialize {
            message: error.to_string(),
        })
    }
}

/// # Errors
///
/// Returns [`PublicDocumentError`] when the JSON payload fails to decode into
/// a typed public document or violates validation rules.
pub fn parse_public_storage_document(
    kind: &WikiStorageDocumentKind,
    data: Value,
) -> Result<PublicStorageDocumentData, PublicDocumentError> {
    let document = serde_json::from_value::<PublicStorageDocumentData>(data).map_err(|error| {
        PublicDocumentError::Serialize {
            message: error.to_string(),
        }
    })?;
    document.ensure_matches_document_kind(kind)?;
    Ok(document)
}

/// # Errors
///
/// Returns [`PublicDocumentError`] when the provided storage document kind is
/// not one of the supported public typed-document kinds.
pub fn default_public_storage_document(
    kind: &WikiStorageDocumentKind,
) -> Result<PublicStorageDocumentData, PublicDocumentError> {
    match kind {
        WikiStorageDocumentKind::PersonalPreferences => Ok(PublicStorageDocumentData::Preferences(
            PublicUserPreferencesDocument {
                preferred_wiki_id: "frwiki".to_string(),
                queue_limit: 25,
                hide_minor: false,
                hide_bots: true,
                editor_types: vec!["anonymous".to_string(), "temporary".to_string()],
                tag_filters: Vec::new(),
            },
        )),
        WikiStorageDocumentKind::SharedRegistry { wiki_id } => Ok(
            PublicStorageDocumentData::Registry(PublicTeamRegistryDocument {
                wiki_id: wiki_id.clone(),
                teams: Vec::new(),
            }),
        ),
        WikiStorageDocumentKind::SharedTeam { wiki_id, team_slug } => Ok(
            PublicStorageDocumentData::Team(PublicTeamDefinitionDocument {
                wiki_id: wiki_id.clone(),
                slug: team_slug.clone(),
                title: team_slug.clone(),
                description: String::new(),
                members: Vec::new(),
            }),
        ),
        WikiStorageDocumentKind::SharedRuleSet {
            wiki_id,
            rule_set_slug,
        } => Ok(PublicStorageDocumentData::RuleSet(PublicRuleSetDocument {
            wiki_id: wiki_id.clone(),
            slug: rule_set_slug.clone(),
            title: rule_set_slug.clone(),
            namespace_allowlist: vec![0],
            tag_filters: Vec::new(),
            hide_minor: false,
            hide_bots: false,
        })),
        WikiStorageDocumentKind::SharedAuditPeriod {
            wiki_id,
            period_slug,
        } => Ok(PublicStorageDocumentData::AuditLedger(
            PublicAuditLedgerDocument {
                wiki_id: wiki_id.clone(),
                period_slug: period_slug.clone(),
                entries: Vec::new(),
            },
        )),
        _ => Err(PublicDocumentError::InvalidDocumentKind {
            message: format!("storage kind `{kind:?}` is not a typed public document"),
        }),
    }
}

/// # Errors
///
/// Returns [`PublicDocumentError`] when document fields are missing, duplicated,
/// or otherwise invalid for public on-wiki storage.
pub fn validate_public_storage_document(
    document: &PublicStorageDocumentData,
) -> Result<(), PublicDocumentError> {
    match document {
        PublicStorageDocumentData::Preferences(value) => {
            if value.preferred_wiki_id.trim().is_empty() {
                return Err(PublicDocumentError::Validation {
                    message: "preferences preferred_wiki_id must not be blank".to_string(),
                });
            }
            if value.queue_limit == 0 {
                return Err(PublicDocumentError::Validation {
                    message: "preferences queue_limit must be greater than zero".to_string(),
                });
            }
        }
        PublicStorageDocumentData::Registry(value) => {
            if value.wiki_id.trim().is_empty() {
                return Err(PublicDocumentError::Validation {
                    message: "registry wiki_id must not be blank".to_string(),
                });
            }
            let mut slugs = BTreeSet::new();
            for entry in &value.teams {
                if entry.slug.trim().is_empty() || entry.title.trim().is_empty() {
                    return Err(PublicDocumentError::Validation {
                        message: "registry entries require non-empty slug and title".to_string(),
                    });
                }
                if !slugs.insert(entry.slug.clone()) {
                    return Err(PublicDocumentError::Validation {
                        message: format!("registry contains duplicate team slug `{}`", entry.slug),
                    });
                }
            }
        }
        PublicStorageDocumentData::Team(value) => {
            if value.wiki_id.trim().is_empty()
                || value.slug.trim().is_empty()
                || value.title.trim().is_empty()
            {
                return Err(PublicDocumentError::Validation {
                    message: "team documents require non-empty wiki_id, slug, and title"
                        .to_string(),
                });
            }
        }
        PublicStorageDocumentData::RuleSet(value) => {
            if value.wiki_id.trim().is_empty()
                || value.slug.trim().is_empty()
                || value.title.trim().is_empty()
            {
                return Err(PublicDocumentError::Validation {
                    message: "rule-set documents require non-empty wiki_id, slug, and title"
                        .to_string(),
                });
            }
            let mut namespaces = BTreeSet::new();
            for namespace in &value.namespace_allowlist {
                if !namespaces.insert(*namespace) {
                    return Err(PublicDocumentError::Validation {
                        message: format!(
                            "rule-set namespace_allowlist contains duplicate namespace `{namespace}`"
                        ),
                    });
                }
            }
        }
        PublicStorageDocumentData::AuditLedger(value) => {
            if value.wiki_id.trim().is_empty() || value.period_slug.trim().is_empty() {
                return Err(PublicDocumentError::Validation {
                    message: "audit ledger requires non-empty wiki_id and period_slug".to_string(),
                });
            }
            for entry in &value.entries {
                if entry.actor.trim().is_empty()
                    || entry.action.trim().is_empty()
                    || entry.summary.trim().is_empty()
                {
                    return Err(PublicDocumentError::Validation {
                        message: "audit entries require non-empty actor, action, and summary"
                            .to_string(),
                    });
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        PublicStorageDocumentData, default_public_storage_document, parse_public_storage_document,
        validate_public_storage_document,
    };
    use crate::errors::PublicDocumentError;
    use crate::wiki_storage::WikiStorageDocumentKind;

    #[test]
    fn default_preferences_document_is_valid() {
        let document =
            default_public_storage_document(&WikiStorageDocumentKind::PersonalPreferences)
                .expect("default document should exist");
        assert!(matches!(
            document,
            PublicStorageDocumentData::Preferences(_)
        ));
        validate_public_storage_document(&document).expect("default document should validate");
    }

    #[test]
    fn parses_registry_document_payload() {
        let document = parse_public_storage_document(
            &WikiStorageDocumentKind::SharedRegistry {
                wiki_id: "frwiki".to_string(),
            },
            json!({
                "type": "registry",
                "document": {
                    "wiki_id": "frwiki",
                    "teams": [{ "slug": "core", "title": "Core" }]
                }
            }),
        )
        .expect("registry payload should parse");

        assert!(matches!(document, PublicStorageDocumentData::Registry(_)));
    }

    #[test]
    fn rejects_duplicate_rule_set_namespaces() {
        let error = parse_public_storage_document(
            &WikiStorageDocumentKind::SharedRuleSet {
                wiki_id: "frwiki".to_string(),
                rule_set_slug: "default".to_string(),
            },
            json!({
                "type": "rule_set",
                "document": {
                    "wiki_id": "frwiki",
                    "slug": "default",
                    "title": "Default",
                    "namespace_allowlist": [0, 0]
                }
            }),
        )
        .expect_err("duplicate namespaces should be rejected");

        assert!(matches!(error, PublicDocumentError::Validation { .. }));
    }

    #[test]
    fn rejects_mismatched_document_kind() {
        let error = parse_public_storage_document(
            &WikiStorageDocumentKind::SharedRegistry {
                wiki_id: "frwiki".to_string(),
            },
            json!({
                "type": "preferences",
                "document": {
                    "preferred_wiki_id": "frwiki",
                    "queue_limit": 10
                }
            }),
        )
        .expect_err("mismatched document kind should be rejected");

        assert!(matches!(
            error,
            PublicDocumentError::InvalidDocumentKind { .. }
        ));
    }
}
