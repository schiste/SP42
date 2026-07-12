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

use sp42_core::scoring_policy::load_embedded_compiled_scoring_policy;
use sp42_core::{
    DEFAULT_PATROL_NAMESPACES, DEFAULT_SCORING_POLICY_REF, WikiConfig, WikiTemplates,
    default_namespace_content_model,
};
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
    let api_url = base.join("/w/api.php").ok()?;
    // Sites with an entity Property namespace (the wikidata family) patrol it
    // by default: without namespace 120 in the allowlist, property edits — the
    // wikibase-property routing this config exists to reach — never enter an
    // unfiltered queue (ADR-0016; PRD-0011).
    let mut namespace_allowlist = DEFAULT_PATROL_NAMESPACES.to_vec();
    if default_namespace_content_model(&api_url, 120).is_some() {
        namespace_allowlist.push(120);
    }
    Some(WikiConfig {
        wiki_id: wiki_id.to_string(),
        display_name: base.host_str().unwrap_or(wiki_id).to_string(),
        api_url,
        eventstreams_url: Url::parse(EVENTSTREAMS_URL).ok()?,
        oauth_authorize_url: Url::parse(OAUTH_AUTHORIZE_URL).ok()?,
        oauth_token_url: Url::parse(OAUTH_TOKEN_URL).ok()?,
        liftwing_url: Some(Url::parse(LIFTWING_URL).ok()?),
        coordination_url: None,
        parsoid_url: Some(base.join("/w/rest.php").ok()?),
        inference_url: None,
        // Shared single source with the patrol filter UI default so a derived
        // wiki surfaces exactly what the filter shows as selected (ADR-0014);
        // extended above for sites with an entity Property namespace.
        namespace_allowlist,
        scoring_policy_ref: DEFAULT_SCORING_POLICY_REF.to_string(),
        scoring: compiled.scoring_config,
        // No hand-tuned citation template for a derived wiki: the tag-citation
        // action refuses rather than inserting a wrong-language template (#91).
        templates: WikiTemplates {
            citation_needed: None,
            bare_url_citation: None,
            citation_concerns: BTreeMap::new(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wikidata_family_defaults_include_the_property_namespace() {
        let wikidata = derive_wiki_config("wikidatawiki").expect("wikidatawiki derives");
        assert!(
            wikidata.namespace_allowlist.contains(&120),
            "property edits must be reachable by default queues"
        );
        let enwiki = derive_wiki_config("enwiki").expect("enwiki derives");
        assert!(!enwiki.namespace_allowlist.contains(&120));
        assert_eq!(
            enwiki.namespace_allowlist,
            DEFAULT_PATROL_NAMESPACES.to_vec()
        );
    }

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
        assert_eq!(config.namespace_allowlist, vec![0, 2, 4, 6, 10, 14]);
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
