//! Error contracts for wiki configuration and registry loading.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("configuration is not valid YAML: {0}")]
    InvalidYaml(#[from] serde_yaml::Error),
    #[error(transparent)]
    ScoringPolicy(#[from] sp42_core::ScoringPolicyError),
    #[error("configuration field `{field}` is invalid: {message}")]
    InvalidField {
        field: &'static str,
        message: String,
    },
    #[error("configuration namespace_allowlist contains duplicate namespace {namespace}")]
    DuplicateNamespace { namespace: i32 },
}

#[derive(Debug, Error)]
pub enum WikiRegistryError {
    #[error("wiki registry requires at least one wiki config")]
    EmptyConfigSet,
    #[error("duplicate wiki config id `{wiki_id}`")]
    DuplicateWikiId { wiki_id: String },
    #[error("{env_var}={wiki_id} does not match any loaded wiki config")]
    UnknownDefaultWikiId {
        env_var: &'static str,
        wiki_id: String,
    },
    #[error("{env_var}={path} did not contain any top-level *.yaml wiki configs")]
    EmptyExplicitConfigDir { env_var: &'static str, path: String },
    #[error("unsupported wiki_id: {wiki_id}")]
    UnknownWikiId { wiki_id: String },
    #[error("embedded frwiki config was invalid: {source}")]
    InvalidEmbeddedConfig { source: ConfigError },
    #[error("failed to read {path}: {message}")]
    ReadDir { path: String, message: String },
    #[error("failed to read an entry from {path}: {message}")]
    ReadEntry { path: String, message: String },
    #[error("failed to read {path}: {message}")]
    ReadFile { path: String, message: String },
    #[error("failed to parse {path}: {source}")]
    ParseConfigFile { path: String, source: ConfigError },
}
