use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sp42_core::{WikiConfig, parse_wiki_config};

const DEFAULT_WIKI_CONFIG_DIR: &str = "configs";
const EMBEDDED_FRWIKI_CONFIG: &str = include_str!("../../../configs/frwiki.yaml");
const SP42_DEFAULT_WIKI_ID: &str = "SP42_DEFAULT_WIKI_ID";
const SP42_WIKI_CONFIG_DIR: &str = "SP42_WIKI_CONFIG_DIR";

#[derive(Clone, Debug)]
pub(crate) struct WikiRegistry {
    inner: Arc<WikiRegistryInner>,
}

#[derive(Debug)]
struct WikiRegistryInner {
    configs: BTreeMap<String, WikiConfig>,
    default_wiki_id: String,
    source: String,
}

impl WikiRegistry {
    pub(crate) fn load() -> Result<Self, String> {
        let explicit_config_dir = env::var_os(SP42_WIKI_CONFIG_DIR).map(PathBuf::from);
        let config_dir = explicit_config_dir
            .clone()
            .unwrap_or_else(|| PathBuf::from(DEFAULT_WIKI_CONFIG_DIR));

        match load_configs_from_dir(&config_dir) {
            Ok(configs) if !configs.is_empty() => Self::from_configs(
                configs,
                configured_default_wiki_id(),
                format!("directory:{}", config_dir.display()),
            ),
            Ok(_) if explicit_config_dir.is_some() => Err(format!(
                "{SP42_WIKI_CONFIG_DIR}={} did not contain any top-level *.yaml wiki configs",
                config_dir.display()
            )),
            Err(error) if explicit_config_dir.is_some() => Err(error),
            Ok(_) | Err(_) => Self::embedded_default(),
        }
    }

    pub(crate) fn embedded_default() -> Result<Self, String> {
        let config = parse_wiki_config(EMBEDDED_FRWIKI_CONFIG)
            .map_err(|error| format!("embedded frwiki config was invalid: {error}"))?;
        Self::from_configs(
            [config],
            configured_default_wiki_id(),
            "embedded:frwiki".to_string(),
        )
    }

    pub(crate) fn from_configs(
        configs: impl IntoIterator<Item = WikiConfig>,
        default_wiki_id: Option<String>,
        source: String,
    ) -> Result<Self, String> {
        let mut by_id = BTreeMap::new();
        for config in configs {
            if by_id.insert(config.wiki_id.clone(), config).is_some() {
                return Err("duplicate wiki config id".to_string());
            }
        }

        let first_wiki_id = by_id
            .keys()
            .next()
            .cloned()
            .ok_or_else(|| "wiki registry requires at least one wiki config".to_string())?;
        let default_wiki_id = default_wiki_id.unwrap_or(first_wiki_id);
        if !by_id.contains_key(&default_wiki_id) {
            return Err(format!(
                "{SP42_DEFAULT_WIKI_ID}={default_wiki_id} does not match any loaded wiki config"
            ));
        }

        Ok(Self {
            inner: Arc::new(WikiRegistryInner {
                configs: by_id,
                default_wiki_id,
                source,
            }),
        })
    }

    pub(crate) fn config(&self, wiki_id: &str) -> Result<WikiConfig, String> {
        self.inner
            .configs
            .get(wiki_id)
            .cloned()
            .ok_or_else(|| format!("unsupported wiki_id: {wiki_id}"))
    }

    #[cfg(test)]
    pub(crate) fn default_config(&self) -> WikiConfig {
        self.inner
            .configs
            .get(&self.inner.default_wiki_id)
            .expect("registry default wiki must be loaded")
            .clone()
    }

    pub(crate) fn default_wiki_id(&self) -> &str {
        &self.inner.default_wiki_id
    }

    pub(crate) fn wiki_count(&self) -> usize {
        self.inner.configs.len()
    }

    pub(crate) fn source(&self) -> &str {
        &self.inner.source
    }
}

fn configured_default_wiki_id() -> Option<String> {
    env::var(SP42_DEFAULT_WIKI_ID)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn load_configs_from_dir(config_dir: &Path) -> Result<Vec<WikiConfig>, String> {
    if !config_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut config_paths = Vec::new();
    for entry in fs::read_dir(config_dir)
        .map_err(|error| format!("failed to read {}: {error}", config_dir.display()))?
    {
        let entry = entry.map_err(|error| {
            format!(
                "failed to read an entry from {}: {error}",
                config_dir.display()
            )
        })?;
        let path = entry.path();
        if path.is_file() && is_yaml_path(&path) {
            config_paths.push(path);
        }
    }
    config_paths.sort();

    config_paths
        .into_iter()
        .map(|path| {
            let source = fs::read_to_string(&path)
                .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
            parse_wiki_config(&source)
                .map_err(|error| format!("failed to parse {}: {error}", path.display()))
        })
        .collect()
}

fn is_yaml_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension, "yaml" | "yml"))
}

#[cfg(test)]
mod tests {
    use super::WikiRegistry;
    use sp42_core::parse_wiki_config;

    #[test]
    fn embedded_default_loads_frwiki() {
        let registry = WikiRegistry::embedded_default().expect("embedded registry should load");

        assert_eq!(registry.default_wiki_id(), "frwiki");
        assert_eq!(registry.default_config().wiki_id, "frwiki");
        assert_eq!(registry.wiki_count(), 1);
    }

    #[test]
    fn rejects_duplicate_wiki_ids() {
        let config = parse_wiki_config(include_str!("../../../configs/frwiki.yaml"))
            .expect("fixture should parse");
        let error = WikiRegistry::from_configs(
            [config.clone(), config],
            Some("frwiki".to_string()),
            "test".to_string(),
        )
        .expect_err("duplicate wiki ids should fail");

        assert!(error.contains("duplicate wiki config id"));
    }

    #[test]
    fn rejects_unknown_default_wiki_id() {
        let config = parse_wiki_config(include_str!("../../../configs/frwiki.yaml"))
            .expect("fixture should parse");
        let error =
            WikiRegistry::from_configs([config], Some("enwiki".to_string()), "test".to_string())
                .expect_err("unknown default should fail");

        assert!(error.contains("SP42_DEFAULT_WIKI_ID=enwiki"));
    }
}
