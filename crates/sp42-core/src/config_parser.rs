//! YAML configuration parsing for per-wiki settings.

use std::collections::BTreeSet;

use crate::errors::ConfigError;
use crate::types::WikiConfig;

/// Parse a single wiki configuration document from YAML.
///
/// # Errors
///
/// Returns [`ConfigError`] when the document is not valid YAML or does not
/// match the [`WikiConfig`] schema.
pub fn parse_wiki_config(source: &str) -> Result<WikiConfig, ConfigError> {
    let config = serde_yaml::from_str(source).map_err(ConfigError::from)?;
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

    #[test]
    fn parses_frwiki_fixture() {
        let source = include_str!("../../../configs/frwiki.yaml");
        let config = parse_wiki_config(source).expect("fixture should parse");

        assert_eq!(config.wiki_id, "frwiki");
        assert_eq!(config.display_name, "French Wikipedia");
        assert_eq!(config.scoring.max_score, 100);
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
    fn rejects_invalid_scoring_bounds() {
        let source = include_str!("../../../configs/frwiki.yaml").replace(
            "base_score: 0\n  max_score: 100",
            "base_score: 100\n  max_score: 10",
        );
        let error = parse_wiki_config(&source).expect_err("invalid scoring bounds should fail");

        assert!(matches!(
            error,
            ConfigError::InvalidField {
                field: "scoring.max_score",
                ..
            }
        ));
    }
}
