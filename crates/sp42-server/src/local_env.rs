use std::fs;
use std::path::{Path, PathBuf};

use sp42_core::{LocalOAuthConfigStatus, LocalOAuthSourceReport};

const LOCAL_ENV_FILE_NAME: &str = ".env.wikimedia.local";
const FALLBACK_ENV_FILE_NAME: &str = ".env";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LocalOAuthConfig {
    source_path: Option<PathBuf>,
    client_application_key: Option<String>,
    client_application_secret: Option<String>,
    access_token: Option<String>,
}

impl LocalOAuthConfig {
    pub fn load() -> Self {
        Self::load_from_candidates(candidate_paths())
    }

    pub fn load_from_candidates<I>(candidates: I) -> Self
    where
        I: IntoIterator<Item = PathBuf>,
    {
        for candidate in candidates {
            let Ok(contents) = fs::read_to_string(&candidate) else {
                continue;
            };

            return parse_local_env(&contents, Some(candidate));
        }

        Self::default()
    }

    pub fn access_token(&self) -> Option<&str> {
        self.access_token.as_deref()
    }

    pub fn client_id(&self) -> Option<&str> {
        self.client_application_key.as_deref()
    }

    pub fn client_secret(&self) -> Option<&str> {
        self.client_application_secret.as_deref()
    }

    pub fn has_confidential_oauth_client(&self) -> bool {
        self.client_id().is_some_and(|value| !value.is_empty())
            && self.client_secret().is_some_and(|value| !value.is_empty())
    }

    pub fn source_path(&self) -> Option<&Path> {
        self.source_path.as_deref()
    }

    pub fn status(&self) -> LocalOAuthConfigStatus {
        LocalOAuthConfigStatus {
            client_id_present: self
                .client_application_key
                .as_ref()
                .is_some_and(|value| !value.is_empty()),
            client_secret_present: self
                .client_application_secret
                .as_ref()
                .is_some_and(|value| !value.is_empty()),
            access_token_present: self
                .access_token
                .as_ref()
                .is_some_and(|value| !value.is_empty()),
        }
    }

    pub fn source_report(&self) -> LocalOAuthSourceReport {
        LocalOAuthSourceReport {
            file_name: LOCAL_ENV_FILE_NAME.to_string(),
            source_path: self
                .source_path
                .as_ref()
                .map(|path| path.display().to_string()),
            loaded_from_source: self.source_path.is_some(),
        }
    }
}

fn parse_local_env(contents: &str, source_path: Option<PathBuf>) -> LocalOAuthConfig {
    let mut config = LocalOAuthConfig {
        source_path,
        ..LocalOAuthConfig::default()
    };

    for raw_line in contents.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let Some((raw_key, raw_value)) = line.split_once('=') else {
            continue;
        };
        let key = raw_key.trim();
        let value = parse_value(raw_value.trim());

        match key {
            "WIKIMEDIA_CLIENT_APPLICATION_KEY" => {
                config.client_application_key = (!value.is_empty()).then_some(value);
            }
            "WIKIMEDIA_CLIENT_APPLICATION_SECRET" => {
                config.client_application_secret = (!value.is_empty()).then_some(value);
            }
            "WIKIMEDIA_ACCESS_TOKEN" => {
                config.access_token = (!value.is_empty()).then_some(value);
            }
            _ => {}
        }
    }

    config
}

fn parse_value(raw_value: &str) -> String {
    let trimmed = raw_value.trim();

    if trimmed.len() >= 2 {
        let first = trimmed.as_bytes()[0];
        let last = trimmed.as_bytes()[trimmed.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return trimmed[1..trimmed.len() - 1].to_string();
        }
    }

    trimmed.to_string()
}

fn candidate_paths() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Ok(current_dir) = std::env::current_dir() {
        extend_unique(
            &mut candidates,
            ancestor_paths(&current_dir, LOCAL_ENV_FILE_NAME),
        );
        extend_unique(
            &mut candidates,
            ancestor_paths(&current_dir, FALLBACK_ENV_FILE_NAME),
        );
    }

    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    extend_unique(
        &mut candidates,
        ancestor_paths(manifest_dir, LOCAL_ENV_FILE_NAME),
    );
    extend_unique(
        &mut candidates,
        ancestor_paths(manifest_dir, FALLBACK_ENV_FILE_NAME),
    );

    candidates
}

fn ancestor_paths(start: &Path, file_name: &str) -> Vec<PathBuf> {
    start.ancestors().map(|path| path.join(file_name)).collect()
}

fn extend_unique(target: &mut Vec<PathBuf>, paths: Vec<PathBuf>) {
    for path in paths {
        if !target.iter().any(|existing| existing == &path) {
            target.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::{LocalOAuthConfig, parse_local_env};

    #[test]
    fn parses_local_env_values() {
        let config = parse_local_env(
            r#"
            # local only
            WIKIMEDIA_CLIENT_APPLICATION_KEY=client-key
            WIKIMEDIA_CLIENT_APPLICATION_SECRET='client-secret'
            WIKIMEDIA_ACCESS_TOKEN="token-value"
            "#,
            Some(PathBuf::from("/tmp/.env.wikimedia.local")),
        );

        assert_eq!(
            config,
            LocalOAuthConfig {
                source_path: Some(PathBuf::from("/tmp/.env.wikimedia.local")),
                client_application_key: Some("client-key".to_string()),
                client_application_secret: Some("client-secret".to_string()),
                access_token: Some("token-value".to_string()),
            }
        );
    }

    #[test]
    fn reports_presence_status() {
        let config = LocalOAuthConfig {
            source_path: None,
            client_application_key: Some("client-key".to_string()),
            client_application_secret: None,
            access_token: Some("token-value".to_string()),
        };

        let status = config.status();

        assert!(status.client_id_present);
        assert!(!status.client_secret_present);
        assert!(status.access_token_present);
    }

    #[test]
    fn reports_source_information() {
        let config = LocalOAuthConfig {
            source_path: Some(PathBuf::from("/tmp/.env.wikimedia.local")),
            client_application_key: None,
            client_application_secret: None,
            access_token: None,
        };

        let report = config.source_report();

        assert_eq!(report.file_name, super::LOCAL_ENV_FILE_NAME);
        assert_eq!(
            report.source_path.as_deref(),
            Some("/tmp/.env.wikimedia.local")
        );
        assert!(report.loaded_from_source);
    }

    #[test]
    fn load_from_candidates_reads_first_available_file() {
        let temp_dir =
            std::env::temp_dir().join(format!("sp42-local-env-test-{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let config_path = temp_dir.join(super::LOCAL_ENV_FILE_NAME);
        fs::write(
            &config_path,
            "WIKIMEDIA_ACCESS_TOKEN=token-from-file\nWIKIMEDIA_CLIENT_APPLICATION_KEY=client-key\n",
        )
        .expect("config file should write");

        let config = LocalOAuthConfig::load_from_candidates([config_path.clone()]);

        assert_eq!(config.access_token(), Some("token-from-file"));
        assert_eq!(config.source_path(), Some(config_path.as_path()));

        let _ = fs::remove_file(&config_path);
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn load_from_candidates_falls_back_to_dot_env() {
        let temp_dir = std::env::temp_dir().join(format!(
            "sp42-local-env-fallback-test-{}",
            std::process::id()
        ));
        let _ = fs::create_dir_all(&temp_dir);
        let config_path = temp_dir.join(super::FALLBACK_ENV_FILE_NAME);
        fs::write(
            &config_path,
            "WIKIMEDIA_ACCESS_TOKEN=token-from-dot-env\nWIKIMEDIA_CLIENT_APPLICATION_KEY=client-key\n",
        )
        .expect("config file should write");

        let config = LocalOAuthConfig::load_from_candidates([config_path.clone()]);

        assert_eq!(config.access_token(), Some("token-from-dot-env"));
        assert_eq!(config.source_path(), Some(config_path.as_path()));

        let _ = fs::remove_file(&config_path);
        let _ = fs::remove_dir_all(&temp_dir);
    }
}
