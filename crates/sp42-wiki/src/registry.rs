use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sp42_core::WikiConfig;

use crate::errors::WikiRegistryError;
use crate::parse_wiki_config;

pub const DEFAULT_WIKI_CONFIG_DIR: &str = "configs";
const EMBEDDED_FRWIKI_CONFIG: &str = include_str!("../../../configs/frwiki.yaml");
pub const SP42_DEFAULT_WIKI_ID: &str = "SP42_DEFAULT_WIKI_ID";
pub const SP42_WIKI_CONFIG_DIR: &str = "SP42_WIKI_CONFIG_DIR";

#[derive(Clone, Debug)]
pub struct WikiRegistry {
    inner: Arc<WikiRegistryInner>,
}

#[derive(Debug)]
struct WikiRegistryInner {
    configs: BTreeMap<String, WikiConfig>,
    default_wiki_id: String,
    source: String,
}

impl WikiRegistry {
    /// Load wiki configs from `SP42_WIKI_CONFIG_DIR` or the repository
    /// `configs/` directory, falling back to the embedded frwiki fixture when
    /// no implicit directory is available.
    ///
    /// # Errors
    ///
    /// Returns [`WikiRegistryError`] when an explicit config directory is
    /// empty/invalid or the configured default wiki is not loaded.
    pub fn load() -> Result<Self, WikiRegistryError> {
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
            Ok(_) if explicit_config_dir.is_some() => {
                Err(WikiRegistryError::EmptyExplicitConfigDir {
                    env_var: SP42_WIKI_CONFIG_DIR,
                    path: config_dir.display().to_string(),
                })
            }
            Err(error) if explicit_config_dir.is_some() => Err(error),
            Ok(_) | Err(_) => Self::embedded_default(),
        }
    }

    /// Build the fallback registry from the embedded frwiki config.
    ///
    /// # Errors
    ///
    /// Returns [`WikiRegistryError`] when the embedded config is invalid or an
    /// environment-provided default wiki ID does not match it.
    pub fn embedded_default() -> Result<Self, WikiRegistryError> {
        let config = parse_wiki_config(EMBEDDED_FRWIKI_CONFIG)
            .map_err(|source| WikiRegistryError::InvalidEmbeddedConfig { source })?;
        Self::from_configs(
            [config],
            configured_default_wiki_id(),
            "embedded:frwiki".to_string(),
        )
    }

    /// Build a registry from already parsed wiki configs.
    ///
    /// # Errors
    ///
    /// Returns [`WikiRegistryError`] when the config set is empty, contains a
    /// duplicate wiki ID, or the requested default is not part of the set.
    pub fn from_configs(
        configs: impl IntoIterator<Item = WikiConfig>,
        default_wiki_id: Option<String>,
        source: String,
    ) -> Result<Self, WikiRegistryError> {
        let mut by_id = BTreeMap::new();
        for config in configs {
            let wiki_id = config.wiki_id.clone();
            if by_id.insert(wiki_id.clone(), config).is_some() {
                return Err(WikiRegistryError::DuplicateWikiId { wiki_id });
            }
        }

        let first_wiki_id = by_id
            .keys()
            .next()
            .cloned()
            .ok_or(WikiRegistryError::EmptyConfigSet)?;
        let default_wiki_id = default_wiki_id.unwrap_or(first_wiki_id);
        if !by_id.contains_key(&default_wiki_id) {
            return Err(WikiRegistryError::UnknownDefaultWikiId {
                env_var: SP42_DEFAULT_WIKI_ID,
                wiki_id: default_wiki_id,
            });
        }

        Ok(Self {
            inner: Arc::new(WikiRegistryInner {
                configs: by_id,
                default_wiki_id,
                source,
            }),
        })
    }

    /// Return a wiki config by ID.
    ///
    /// # Errors
    ///
    /// Returns [`WikiRegistryError::UnknownWikiId`] when the wiki is not
    /// loaded in the registry.
    pub fn config(&self, wiki_id: &str) -> Result<WikiConfig, WikiRegistryError> {
        self.inner
            .configs
            .get(wiki_id)
            .cloned()
            .ok_or_else(|| WikiRegistryError::UnknownWikiId {
                wiki_id: wiki_id.to_string(),
            })
    }

    /// # Panics
    ///
    /// Panics only if the registry invariant is broken and the stored default
    /// wiki ID no longer points to a loaded config.
    #[must_use]
    pub fn default_config(&self) -> WikiConfig {
        self.inner
            .configs
            .get(&self.inner.default_wiki_id)
            .expect("registry default wiki must be loaded")
            .clone()
    }

    #[must_use]
    pub fn default_wiki_id(&self) -> &str {
        &self.inner.default_wiki_id
    }

    #[must_use]
    pub fn wiki_count(&self) -> usize {
        self.inner.configs.len()
    }

    #[must_use]
    pub fn source(&self) -> &str {
        &self.inner.source
    }

    #[must_use]
    pub fn wiki_ids(&self) -> Vec<String> {
        self.inner.configs.keys().cloned().collect()
    }
}

fn configured_default_wiki_id() -> Option<String> {
    env::var(SP42_DEFAULT_WIKI_ID)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Load every top-level YAML wiki config in a directory.
///
/// # Errors
///
/// Returns [`WikiRegistryError`] when the directory cannot be read or any
/// config file cannot be read/parsed.
pub fn load_configs_from_dir(config_dir: &Path) -> Result<Vec<WikiConfig>, WikiRegistryError> {
    if !config_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut config_paths = Vec::new();
    for entry in fs::read_dir(config_dir).map_err(|error| WikiRegistryError::ReadDir {
        path: config_dir.display().to_string(),
        message: error.to_string(),
    })? {
        let entry = entry.map_err(|error| WikiRegistryError::ReadEntry {
            path: config_dir.display().to_string(),
            message: error.to_string(),
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
            let source =
                fs::read_to_string(&path).map_err(|error| WikiRegistryError::ReadFile {
                    path: path.display().to_string(),
                    message: error.to_string(),
                })?;
            parse_wiki_config(&source).map_err(|source| WikiRegistryError::ParseConfigFile {
                path: path.display().to_string(),
                source,
            })
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
    use std::fs;
    use std::path::PathBuf;

    use crate::parse_wiki_config;

    use super::WikiRegistry;

    fn temp_config_dir(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "sp42-wiki-{label}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("temp config directory should be created");
        path
    }

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

        assert!(error.to_string().contains("duplicate wiki config id"));
    }

    #[test]
    fn rejects_unknown_default_wiki_id() {
        let config = parse_wiki_config(include_str!("../../../configs/frwiki.yaml"))
            .expect("fixture should parse");
        let error =
            WikiRegistry::from_configs([config], Some("enwiki".to_string()), "test".to_string())
                .expect_err("unknown default should fail");

        assert!(error.to_string().contains("SP42_DEFAULT_WIKI_ID=enwiki"));
    }

    #[test]
    fn loads_multiple_wiki_configs_from_directory() {
        let temp_dir = temp_config_dir("multiwiki");
        fs::write(
            temp_dir.join("frwiki.yaml"),
            include_str!("../../../configs/frwiki.yaml"),
        )
        .expect("frwiki config should write");
        fs::write(
            temp_dir.join("testwiki.yaml"),
            include_str!("../../../fixtures/testwiki.yaml"),
        )
        .expect("testwiki config should write");

        let configs = super::load_configs_from_dir(&temp_dir).expect("configs should load");
        let registry = WikiRegistry::from_configs(
            configs,
            Some("testwiki".to_string()),
            "test-directory".to_string(),
        )
        .expect("registry should build");

        assert_eq!(registry.default_wiki_id(), "testwiki");
        assert_eq!(registry.wiki_ids(), vec!["frwiki", "testwiki"]);

        fs::remove_dir_all(temp_dir).expect("temp config directory should clean up");
    }
}
