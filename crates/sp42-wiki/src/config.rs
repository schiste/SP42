//! YAML configuration parsing for per-wiki settings.

use std::collections::BTreeSet;

use crate::errors::ConfigError;
use serde::Deserialize;
use sp42_core::scoring_policy::load_embedded_compiled_scoring_policy;
use sp42_core::{WikiConfig, WikiTemplates};
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct RawWikiConfig {
    wiki_id: String,
    display_name: String,
    api_url: Url,
    eventstreams_url: Url,
    oauth_authorize_url: Url,
    oauth_token_url: Url,
    liftwing_url: Option<Url>,
    coordination_url: Option<Url>,
    parsoid_url: Option<Url>,
    #[serde(default)]
    namespace_allowlist: Vec<i32>,
    #[serde(default = "default_scoring_policy_ref")]
    scoring_policy_ref: String,
    #[serde(default)]
    templates: WikiTemplates,
}

fn default_scoring_policy_ref() -> String {
    "active/frwiki-vandalism".to_string()
}

/// Parse a single wiki configuration document from YAML.
///
/// # Errors
///
/// Returns [`ConfigError`] when the document is not valid YAML or does not
/// match the [`WikiConfig`] schema.
pub fn parse_wiki_config(source: &str) -> Result<WikiConfig, ConfigError> {
    let raw = serde_yaml::from_str::<RawWikiConfig>(source).map_err(ConfigError::from)?;
    ensure_non_empty("wiki_id", &raw.wiki_id)?;
    ensure_non_empty("display_name", &raw.display_name)?;
    let compiled_policy = load_embedded_compiled_scoring_policy(&raw.scoring_policy_ref)?;
    if compiled_policy.wiki_id != raw.wiki_id {
        return Err(ConfigError::InvalidField {
            field: "scoring_policy_ref",
            message: format!(
                "policy wiki_id `{}` does not match config wiki_id `{}`",
                compiled_policy.wiki_id, raw.wiki_id
            ),
        });
    }
    let config = WikiConfig {
        wiki_id: raw.wiki_id,
        display_name: raw.display_name,
        api_url: raw.api_url,
        eventstreams_url: raw.eventstreams_url,
        oauth_authorize_url: raw.oauth_authorize_url,
        oauth_token_url: raw.oauth_token_url,
        liftwing_url: raw.liftwing_url,
        coordination_url: raw.coordination_url,
        parsoid_url: raw.parsoid_url,
        namespace_allowlist: raw.namespace_allowlist,
        scoring_policy_ref: raw.scoring_policy_ref,
        scoring: compiled_policy.scoring_config,
        templates: raw.templates,
    };
    validate_config(config)
}

fn validate_config(config: WikiConfig) -> Result<WikiConfig, ConfigError> {
    ensure_non_empty("wiki_id", &config.wiki_id)?;
    ensure_non_empty("display_name", &config.display_name)?;
    ensure_url_scheme("api_url", &config.api_url, &["http", "https"])?;
    ensure_url_scheme(
        "eventstreams_url",
        &config.eventstreams_url,
        &["http", "https"],
    )?;
    ensure_url_scheme(
        "oauth_authorize_url",
        &config.oauth_authorize_url,
        &["http", "https"],
    )?;
    ensure_url_scheme(
        "oauth_token_url",
        &config.oauth_token_url,
        &["http", "https"],
    )?;
    ensure_optional_url_scheme(
        "liftwing_url",
        config.liftwing_url.as_ref(),
        &["http", "https"],
    )?;
    ensure_optional_url_scheme(
        "coordination_url",
        config.coordination_url.as_ref(),
        &["http", "https", "ws", "wss"],
    )?;
    ensure_optional_url_scheme(
        "parsoid_url",
        config.parsoid_url.as_ref(),
        &["http", "https"],
    )?;

    if !config.api_url.path().ends_with("/api.php") {
        return Err(ConfigError::InvalidField {
            field: "api_url",
            message: "expected MediaWiki api.php endpoint".to_string(),
        });
    }

    if config.scoring.max_score < config.scoring.base_score {
        return Err(ConfigError::InvalidField {
            field: "scoring.max_score",
            message: "must be greater than or equal to scoring.base_score".to_string(),
        });
    }

    let mut seen_namespaces = BTreeSet::new();
    for namespace in &config.namespace_allowlist {
        if !seen_namespaces.insert(*namespace) {
            return Err(ConfigError::DuplicateNamespace {
                namespace: *namespace,
            });
        }
    }

    Ok(config)
}

fn ensure_non_empty(field: &'static str, value: &str) -> Result<(), ConfigError> {
    if value.trim().is_empty() {
        return Err(ConfigError::InvalidField {
            field,
            message: "value is required".to_string(),
        });
    }

    Ok(())
}

fn ensure_url_scheme(
    field: &'static str,
    url: &url::Url,
    allowed_schemes: &[&str],
) -> Result<(), ConfigError> {
    if allowed_schemes.contains(&url.scheme()) {
        return Ok(());
    }

    Err(ConfigError::InvalidField {
        field,
        message: format!(
            "scheme `{}` is not allowed; expected one of {}",
            url.scheme(),
            allowed_schemes.join(", ")
        ),
    })
}

fn ensure_optional_url_scheme(
    field: &'static str,
    url: Option<&url::Url>,
    allowed_schemes: &[&str],
) -> Result<(), ConfigError> {
    if let Some(url) = url {
        ensure_url_scheme(field, url, allowed_schemes)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_wiki_config;
    use crate::errors::ConfigError;
    use url::Url;

    #[test]
    fn parses_optional_parsoid_url() {
        let yaml = r"
wiki_id: frwiki
display_name: French Wikipedia
api_url: https://fr.wikipedia.org/w/api.php
eventstreams_url: https://stream.wikimedia.org/v2/stream/recentchange
oauth_authorize_url: https://meta.wikimedia.org/w/rest.php/oauth2/authorize
oauth_token_url: https://meta.wikimedia.org/w/rest.php/oauth2/access_token
liftwing_url:
coordination_url:
parsoid_url: https://fr.wikipedia.org/w/rest.php
namespace_allowlist: [0]
scoring_policy_ref: active/frwiki-vandalism
";
        let config = parse_wiki_config(yaml).expect("config with parsoid_url should parse");
        assert_eq!(
            config.parsoid_url.as_ref().map(Url::as_str),
            Some("https://fr.wikipedia.org/w/rest.php")
        );
    }

    #[test]
    fn parsoid_url_defaults_to_none_when_absent() {
        let yaml = r"
wiki_id: frwiki
display_name: French Wikipedia
api_url: https://fr.wikipedia.org/w/api.php
eventstreams_url: https://stream.wikimedia.org/v2/stream/recentchange
oauth_authorize_url: https://meta.wikimedia.org/w/rest.php/oauth2/authorize
oauth_token_url: https://meta.wikimedia.org/w/rest.php/oauth2/access_token
liftwing_url:
coordination_url:
namespace_allowlist: [0]
scoring_policy_ref: active/frwiki-vandalism
";
        let config = parse_wiki_config(yaml).expect("config without parsoid_url should parse");
        assert_eq!(config.parsoid_url, None);
    }

    #[test]
    fn rejects_parsoid_url_with_unsupported_scheme() {
        let yaml = r"
wiki_id: frwiki
display_name: French Wikipedia
api_url: https://fr.wikipedia.org/w/api.php
eventstreams_url: https://stream.wikimedia.org/v2/stream/recentchange
oauth_authorize_url: https://meta.wikimedia.org/w/rest.php/oauth2/authorize
oauth_token_url: https://meta.wikimedia.org/w/rest.php/oauth2/access_token
liftwing_url:
coordination_url:
parsoid_url: ftp://fr.wikipedia.org/w/rest.php
namespace_allowlist: [0]
scoring_policy_ref: active/frwiki-vandalism
";
        let error = parse_wiki_config(yaml).expect_err("ftp parsoid_url should be rejected");
        assert!(matches!(
            error,
            ConfigError::InvalidField { field: "parsoid_url", .. }
        ));
    }

    #[test]
    fn parses_frwiki_fixture() {
        let source = include_str!("../../../configs/frwiki.yaml");
        let config = parse_wiki_config(source).expect("fixture should parse");

        assert_eq!(config.wiki_id, "frwiki");
        assert_eq!(config.display_name, "French Wikipedia");
        assert_eq!(config.scoring.max_score, 100);
        assert_eq!(config.scoring_policy_ref, "active/frwiki-vandalism");
        assert_eq!(config.scoring.identity.contribution_cap, Some(25));
    }

    #[test]
    fn rejects_blank_wiki_id() {
        let source = include_str!("../../../configs/frwiki.yaml")
            .replace("wiki_id: frwiki", "wiki_id: \"  \"");
        let error = parse_wiki_config(&source).expect_err("blank wiki_id should fail");

        assert!(matches!(
            error,
            ConfigError::InvalidField {
                field: "wiki_id",
                ..
            }
        ));
    }

    #[test]
    fn rejects_duplicate_namespace_allowlist_entries() {
        let source = include_str!("../../../configs/frwiki.yaml").replace(
            "namespace_allowlist:\n  - 0\n  - 2\n  - 4\n  - 6\n  - 10\n  - 14",
            "namespace_allowlist:\n  - 0\n  - 2\n  - 2\n  - 14",
        );
        let error = parse_wiki_config(&source).expect_err("duplicate namespaces should fail");

        assert!(matches!(
            error,
            ConfigError::DuplicateNamespace { namespace: 2 }
        ));
    }

    #[test]
    fn rejects_unsupported_url_schemes() {
        let source = include_str!("../../../configs/frwiki.yaml").replace(
            "api_url: https://fr.wikipedia.org/w/api.php",
            "api_url: ftp://fr.wikipedia.org/w/api.php",
        );
        let error = parse_wiki_config(&source).expect_err("unsupported schemes should fail");

        assert!(matches!(
            error,
            ConfigError::InvalidField {
                field: "api_url",
                ..
            }
        ));
    }

    #[test]
    fn rejects_unknown_scoring_policy_reference() {
        let source = include_str!("../../../configs/frwiki.yaml").replace(
            "scoring_policy_ref: active/frwiki-vandalism",
            "scoring_policy_ref: active/unknown",
        );
        let error = parse_wiki_config(&source).expect_err("unknown policy ref should fail");

        assert!(matches!(error, ConfigError::ScoringPolicy(_)));
    }

    #[test]
    fn bare_url_citation_parses_when_present() {
        let yaml = r#"
wiki_id: frwiki
display_name: French Wikipedia
api_url: https://fr.wikipedia.org/w/api.php
eventstreams_url: https://stream.wikimedia.org/v2/stream/recentchange
oauth_authorize_url: https://meta.wikimedia.org/w/rest.php/oauth2/authorize
oauth_token_url: https://meta.wikimedia.org/w/rest.php/oauth2/access_token
liftwing_url:
coordination_url:
namespace_allowlist: [0]
scoring_policy_ref: active/frwiki-vandalism
templates:
  citation_needed: "Citation needed"
  bare_url_citation: "cite web"
"#;
        let config = parse_wiki_config(yaml).expect("config with bare_url_citation should parse");
        assert_eq!(config.templates.bare_url_citation.as_deref(), Some("cite web"));
    }

    #[test]
    fn bare_url_citation_defaults_to_none_when_absent() {
        let source = include_str!("../../../configs/frwiki.yaml");
        let config = parse_wiki_config(source).expect("fixture should parse");
        assert_eq!(config.templates.bare_url_citation, None);
    }

    #[test]
    fn testwiki_fixture_enables_bare_url_citation() {
        let source = include_str!("../../../fixtures/testwiki.yaml");
        let config = parse_wiki_config(source).expect("testwiki fixture should parse");
        assert_eq!(config.templates.bare_url_citation.as_deref(), Some("cite web"));
    }
}
