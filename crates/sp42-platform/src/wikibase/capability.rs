use crate::types::ContentModel;

/// Which content-model-specific features apply to a revision (ADR-0016 Decision 5).
/// A property of the *content*, not the account — a separate axis from
/// `derive_wiki_capability_profile` (sp42-wiki), which is untouched.
/// Gated features are NOT invoked (not invoked-and-discarded).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[allow(clippy::struct_excessive_bools)] // https://github.com/schiste/SP42/blob/main/docs/platform/adr/0016-wikidata-entity-content-model.md: Five independent capability flags
pub struct ContentCapabilityProfile {
    pub media_reference_extraction: bool,
    pub talk_page_warning_parsing: bool,
    pub citation_extraction: bool,
    /// `LiftWing` revertrisk — Wikipedia-wikitext-trained; skipped, never faked (D7).
    pub revertrisk_scoring: bool,
    pub entity_diff: bool,
}

#[must_use]
pub fn derive_content_capability_profile(model: &ContentModel) -> ContentCapabilityProfile {
    match model {
        ContentModel::Wikitext => ContentCapabilityProfile {
            media_reference_extraction: true,
            talk_page_warning_parsing: true,
            citation_extraction: true,
            revertrisk_scoring: true,
            entity_diff: false,
        },
        ContentModel::WikibaseItem | ContentModel::WikibaseProperty => ContentCapabilityProfile {
            media_reference_extraction: false,
            talk_page_warning_parsing: false,
            citation_extraction: false,
            revertrisk_scoring: false,
            entity_diff: true,
        },
        ContentModel::Other(_) => ContentCapabilityProfile {
            media_reference_extraction: false,
            talk_page_warning_parsing: false,
            citation_extraction: false,
            revertrisk_scoring: false,
            entity_diff: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wikitext_enables_all_wikitext_signals() {
        let profile = derive_content_capability_profile(&ContentModel::Wikitext);
        assert!(profile.media_reference_extraction);
        assert!(profile.talk_page_warning_parsing);
        assert!(profile.citation_extraction);
        assert!(profile.revertrisk_scoring);
        assert!(!profile.entity_diff);
    }

    #[test]
    fn entity_models_gate_wikitext_signals_off_and_enable_entity_diff() {
        for model in [ContentModel::WikibaseItem, ContentModel::WikibaseProperty] {
            let profile = derive_content_capability_profile(&model);
            assert!(!profile.media_reference_extraction);
            assert!(!profile.talk_page_warning_parsing);
            assert!(!profile.citation_extraction);
            assert!(
                !profile.revertrisk_scoring,
                "ADR-0016 D7: no LiftWing for entities"
            );
            assert!(profile.entity_diff);
        }
    }

    #[test]
    fn unknown_models_degrade_to_text_with_no_entity_diff() {
        let profile = derive_content_capability_profile(&ContentModel::Other("Scribunto".into()));
        assert!(!profile.entity_diff);
        assert!(!profile.revertrisk_scoring); // trained on Wikipedia wikitext only
    }
}
