//! User metadata parsing and local warning heuristics.

use std::collections::{BTreeMap, VecDeque};

use crate::types::{UserRiskProfile, WarningLevel};

const FINAL_PATTERNS: [&str; 4] = [
    "final",
    "dernier avertissement",
    "ultime avertissement",
    "4im",
];
const LEVEL4_PATTERNS: [&str; 4] = ["niveau 4", "level 4", "test4", "vandalism4"];
const LEVEL3_PATTERNS: [&str; 4] = ["niveau 3", "level 3", "test3", "vandalism3"];
const LEVEL2_PATTERNS: [&str; 4] = ["niveau 2", "level 2", "test2", "vandalism2"];
const LEVEL1_PATTERNS: [&str; 4] = ["niveau 1", "level 1", "test1", "vandalism1"];
const VANDALISM_PATTERNS: [&str; 5] = ["vandalisme", "vandalism", "uw-vandalism", "test4", "test3"];

#[must_use]
pub fn parse_warning_level(talk_page_wikitext: &str) -> WarningLevel {
    let normalized = talk_page_wikitext.to_ascii_lowercase();

    if contains_any(&normalized, &FINAL_PATTERNS) {
        WarningLevel::Final
    } else if contains_any(&normalized, &LEVEL4_PATTERNS) {
        WarningLevel::Level4
    } else if contains_any(&normalized, &LEVEL3_PATTERNS) {
        WarningLevel::Level3
    } else if contains_any(&normalized, &LEVEL2_PATTERNS) {
        WarningLevel::Level2
    } else if contains_any(&normalized, &LEVEL1_PATTERNS) {
        WarningLevel::Level1
    } else {
        WarningLevel::None
    }
}

#[must_use]
pub fn count_warning_templates(talk_page_wikitext: &str) -> u32 {
    let normalized = talk_page_wikitext.to_ascii_lowercase();
    extract_template_bodies(&normalized)
        .into_iter()
        .filter(|template_body| is_warning_template(template_body))
        .count()
        .try_into()
        .unwrap_or(u32::MAX)
}

#[must_use]
pub fn build_user_risk_profile(talk_page_wikitext: &str) -> UserRiskProfile {
    let normalized = talk_page_wikitext.to_ascii_lowercase();

    UserRiskProfile {
        warning_level: parse_warning_level(talk_page_wikitext),
        warning_count: count_warning_templates(talk_page_wikitext),
        has_recent_vandalism_templates: contains_any(&normalized, &VANDALISM_PATTERNS),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserRiskCache {
    capacity: usize,
    order: VecDeque<String>,
    entries: BTreeMap<String, UserRiskProfile>,
}

impl UserRiskCache {
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            order: VecDeque::new(),
            entries: BTreeMap::new(),
        }
    }

    pub fn get(&mut self, username: &str) -> Option<UserRiskProfile> {
        let profile = self.entries.get(username).cloned()?;
        self.touch(username);
        Some(profile)
    }

    pub fn insert(&mut self, username: String, profile: UserRiskProfile) {
        if self.capacity == 0 {
            return;
        }

        self.touch(&username);
        self.entries.insert(username, profile);

        while self.entries.len() > self.capacity {
            if let Some(evicted_username) = self.order.pop_front() {
                self.entries.remove(&evicted_username);
            }
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn touch(&mut self, username: &str) {
        self.order.retain(|candidate| candidate != username);
        self.order.push_back(username.to_string());
    }
}

fn contains_any(haystack: &str, patterns: &[&str]) -> bool {
    patterns.iter().any(|pattern| haystack.contains(pattern))
}

fn is_warning_template(template_body: &str) -> bool {
    template_body.contains("avertissement")
        || contains_any(template_body, &FINAL_PATTERNS)
        || contains_any(template_body, &LEVEL4_PATTERNS)
        || contains_any(template_body, &LEVEL3_PATTERNS)
        || contains_any(template_body, &LEVEL2_PATTERNS)
        || contains_any(template_body, &LEVEL1_PATTERNS)
        || contains_any(template_body, &VANDALISM_PATTERNS)
}

fn extract_template_bodies(wikitext: &str) -> Vec<&str> {
    let mut bodies = Vec::new();
    let mut remaining = wikitext;

    while let Some(start) = remaining.find("{{") {
        let after_start = &remaining[start + 2..];
        let Some(end) = after_start.find("}}") else {
            break;
        };
        bodies.push(&after_start[..end]);
        remaining = &after_start[end + 2..];
    }

    bodies
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::{
        UserRiskCache, build_user_risk_profile, count_warning_templates, parse_warning_level,
    };
    use crate::types::{UserRiskProfile, WarningLevel};

    #[test]
    fn detects_highest_warning_level_present() {
        let level = parse_warning_level(
            "== Mars ==\n{{Avertissement niveau 2}}\n{{Dernier avertissement}}",
        );

        assert_eq!(level, WarningLevel::Final);
    }

    #[test]
    fn counts_warning_mentions() {
        let count = count_warning_templates(
            "{{Avertissement niveau 1}}\n{{Avertissement niveau 2}}\ntexte",
        );

        assert_eq!(count, 2);
    }

    #[test]
    fn counts_english_warning_templates() {
        let count = count_warning_templates("{{uw-vandalism1}}\n{{uw-vandalism4im}}");

        assert_eq!(count, 2);
    }

    #[test]
    fn builds_user_risk_profile() {
        let profile = build_user_risk_profile(
            "== Avril ==\n{{Avertissement niveau 3 pour vandalisme}}\n{{uw-vandalism4}}",
        );

        assert_eq!(profile.warning_level, WarningLevel::Level4);
        assert!(profile.has_recent_vandalism_templates);
    }

    #[test]
    fn cache_returns_inserted_profile() {
        let mut cache = UserRiskCache::new(2);
        let profile = UserRiskProfile {
            warning_level: WarningLevel::Level2,
            warning_count: 2,
            has_recent_vandalism_templates: false,
        };

        cache.insert("Example".to_string(), profile.clone());

        assert_eq!(cache.get("Example"), Some(profile));
    }

    #[test]
    fn cache_evicts_least_recently_used_profile() {
        let mut cache = UserRiskCache::new(2);

        cache.insert(
            "First".to_string(),
            UserRiskProfile {
                warning_level: WarningLevel::Level1,
                warning_count: 1,
                has_recent_vandalism_templates: false,
            },
        );
        cache.insert(
            "Second".to_string(),
            UserRiskProfile {
                warning_level: WarningLevel::Level2,
                warning_count: 2,
                has_recent_vandalism_templates: false,
            },
        );
        let _ = cache.get("First");
        cache.insert(
            "Third".to_string(),
            UserRiskProfile {
                warning_level: WarningLevel::Level3,
                warning_count: 3,
                has_recent_vandalism_templates: true,
            },
        );

        assert!(cache.get("Second").is_none());
        assert!(cache.get("First").is_some());
        assert!(cache.get("Third").is_some());
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn zero_capacity_cache_stays_empty() {
        let mut cache = UserRiskCache::new(0);

        cache.insert(
            "Ignored".to_string(),
            UserRiskProfile {
                warning_level: WarningLevel::Level1,
                warning_count: 1,
                has_recent_vandalism_templates: false,
            },
        );

        assert!(cache.is_empty());
        assert!(cache.get("Ignored").is_none());
    }

    #[test]
    fn reinserting_existing_username_refreshes_profile_without_duplication() {
        let mut cache = UserRiskCache::new(2);

        cache.insert(
            "Example".to_string(),
            UserRiskProfile {
                warning_level: WarningLevel::Level1,
                warning_count: 1,
                has_recent_vandalism_templates: false,
            },
        );
        cache.insert(
            "Example".to_string(),
            UserRiskProfile {
                warning_level: WarningLevel::Level4,
                warning_count: 4,
                has_recent_vandalism_templates: true,
            },
        );

        assert_eq!(cache.len(), 1);
        assert_eq!(
            cache.get("Example"),
            Some(UserRiskProfile {
                warning_level: WarningLevel::Level4,
                warning_count: 4,
                has_recent_vandalism_templates: true,
            })
        );
    }

    proptest! {
        #[test]
        fn property_cache_len_never_exceeds_capacity(
            usernames in prop::collection::vec("[A-Za-z]{1,8}", 1..32),
            capacity in 0usize..8,
        ) {
            let mut cache = UserRiskCache::new(capacity);

            for (index, username) in usernames.iter().enumerate() {
                cache.insert(
                    username.clone(),
                    UserRiskProfile {
                        warning_level: WarningLevel::Level1,
                        warning_count: u32::try_from(index).unwrap_or(u32::MAX),
                        has_recent_vandalism_templates: index % 2 == 0,
                    },
                );
            }

            prop_assert!(cache.len() <= capacity);
        }
    }
}
