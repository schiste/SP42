//! Authoritative Wikimedia site list, embedded at build time (ADR-0014).
//!
//! SP42 works with **any** Wikimedia project without a hand-written config: the
//! vendored `data/wikimedia-sites.json` snapshot of the `SiteMatrix` API (refresh
//! via `scripts/sync-wikis.sh`) maps each `dbname` (which is SP42's `wiki_id`) to
//! its base URL. From that we derive a [`WikiConfig`] — deriving `api_url` /
//! `parsoid_url` from the base, using the shared Wikimedia endpoints (identical
//! across every project) and the universal default scoring policy.
//!
//! SSRF-safe: hosts only ever come from this vendored authoritative data, never
//! from raw caller input — an unknown `wiki_id` yields `None`.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use sp42_core::WikiConfig;
use sp42_core::WikiTemplates;
use sp42_core::scoring_policy::load_embedded_compiled_scoring_policy;
use url::Url;

const EMBEDDED_SITES_JSON: &str = include_str!("../data/wikimedia-sites.json");

/// `dbname -> base url`, e.g. `"frwiki" -> "https://fr.wikipedia.org"`.
static SITES: LazyLock<BTreeMap<String, String>> = LazyLock::new(|| {
    serde_json::from_str(EMBEDDED_SITES_JSON).expect("embedded wikimedia-sites.json must be valid")
});

// Shared endpoints — identical for every Wikimedia project.
const EVENTSTREAMS_URL: &str = "https://stream.wikimedia.org/v2/stream/recentchange";
const OAUTH_AUTHORIZE_URL: &str = "https://meta.wikimedia.org/w/rest.php/oauth2/authorize";
const OAUTH_TOKEN_URL: &str = "https://meta.wikimedia.org/w/rest.php/oauth2/access_token";
const LIFTWING_URL: &str =
    "https://api.wikimedia.org/service/lw/inference/v1/models/revertrisk-language-agnostic:predict";
/// Universal baseline applied to any wiki without a hand-tuned config.
pub const DEFAULT_SCORING_POLICY_REF: &str = "active/default-language-agnostic";
/// Patrol-relevant content namespaces for dynamically-derived wikis: main (0),
/// project (4), file (6), template (10), category (14). Broader than article-
/// only so e.g. Commons File edits aren't dropped by the namespace filter.
const DERIVED_NAMESPACE_ALLOWLIST: [i32; 5] = [0, 4, 6, 10, 14];

/// Whether `wiki_id` is a known Wikimedia project in the embedded site list.
#[must_use]
pub fn is_known_wiki(wiki_id: &str) -> bool {
    SITES.contains_key(wiki_id)
}

/// Number of Wikimedia projects in the embedded site list.
#[must_use]
pub fn known_wiki_count() -> usize {
    SITES.len()
}

/// All known Wikimedia `wiki_id`s (sorted), for the wiki picker.
#[must_use]
pub fn known_wiki_ids() -> Vec<String> {
    SITES.keys().cloned().collect()
}

/// Derive a [`WikiConfig`] for any Wikimedia project in the embedded site list.
///
/// Returns `None` when `wiki_id` is not a known Wikimedia `dbname` — this is the
/// SSRF boundary: every URL in the result is built from vendored authoritative
/// data, never from arbitrary caller input.
#[must_use]
pub fn derive_wiki_config(wiki_id: &str) -> Option<WikiConfig> {
    let base = Url::parse(SITES.get(wiki_id)?).ok()?;
    let compiled = load_embedded_compiled_scoring_policy(DEFAULT_SCORING_POLICY_REF).ok()?;
    Some(WikiConfig {
        wiki_id: wiki_id.to_string(),
        display_name: base.host_str().unwrap_or(wiki_id).to_string(),
        api_url: base.join("/w/api.php").ok()?,
        eventstreams_url: Url::parse(EVENTSTREAMS_URL).ok()?,
        oauth_authorize_url: Url::parse(OAUTH_AUTHORIZE_URL).ok()?,
        oauth_token_url: Url::parse(OAUTH_TOKEN_URL).ok()?,
        liftwing_url: Some(Url::parse(LIFTWING_URL).ok()?),
        coordination_url: None,
        parsoid_url: Some(base.join("/w/rest.php").ok()?),
        inference_url: None,
        namespace_allowlist: DERIVED_NAMESPACE_ALLOWLIST.to_vec(),
        scoring_policy_ref: DEFAULT_SCORING_POLICY_REF.to_string(),
        scoring: compiled.scoring_config,
        templates: WikiTemplates {
            citation_needed: "Citation needed".to_string(),
            bare_url_citation: None,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_list_loads_and_has_core_wikis() {
        assert!(
            known_wiki_count() > 500,
            "expected the full SiteMatrix snapshot"
        );
        for id in [
            "enwiki",
            "frwiki",
            "dewiki",
            "commonswiki",
            "wikidatawiki",
            "eswiktionary",
        ] {
            assert!(is_known_wiki(id), "{id} should be known");
        }
        assert!(!is_known_wiki("not-a-wiki"));
        assert!(!is_known_wiki("evil.example.com"));
    }

    #[test]
    fn derives_wikipedia_config() {
        let config = derive_wiki_config("dewiki").expect("dewiki should derive");
        assert_eq!(config.wiki_id, "dewiki");
        assert_eq!(
            config.api_url.as_str(),
            "https://de.wikipedia.org/w/api.php"
        );
        assert_eq!(
            config.parsoid_url.as_ref().map(Url::as_str),
            Some("https://de.wikipedia.org/w/rest.php")
        );
        assert_eq!(config.scoring_policy_ref, DEFAULT_SCORING_POLICY_REF);
        assert_eq!(config.namespace_allowlist, vec![0, 4, 6, 10, 14]);
        // shared endpoints derive to the central Wikimedia services
        assert_eq!(
            config.oauth_authorize_url.as_str(),
            "https://meta.wikimedia.org/w/rest.php/oauth2/authorize"
        );
    }

    #[test]
    fn derives_sister_project_config() {
        let config = derive_wiki_config("commonswiki").expect("commonswiki should derive");
        assert_eq!(
            config.api_url.as_str(),
            "https://commons.wikimedia.org/w/api.php"
        );
    }

    #[test]
    fn unknown_wiki_does_not_derive() {
        assert!(derive_wiki_config("definitely-not-a-wiki").is_none());
    }
}
