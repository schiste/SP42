use sp42_core::{CoordinationSnapshot, ServerDebugSummary};

use super::{auth, coordination, debug, pwa};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserBootstrapSnapshot {
    pub pwa: pwa::PwaEnvironmentStatus,
    pub dev_auth_bootstrap: auth::DevAuthBootstrapStatus,
    pub coordination: CoordinationSnapshot,
    pub runtime: debug::RuntimeDebugStatus,
    pub server_debug: ServerDebugSummary,
    pub errors: Vec<String>,
}

#[cfg(target_arch = "wasm32")]
pub async fn collect_browser_bootstrap_snapshot() -> BrowserBootstrapSnapshot {
    let mut errors = Vec::new();

    let pwa = pwa::initialize_pwa().await;

    let dev_auth_bootstrap = match auth::fetch_dev_auth_bootstrap_status().await {
        Ok(status) => status,
        Err(error) => {
            errors.push(format!("dev auth bootstrap: {error}"));
            auth::preview_dev_auth_bootstrap_status()
        }
    };

    let coordination = match coordination::fetch_coordination_snapshot().await {
        Ok(snapshot) => snapshot,
        Err(error) => {
            errors.push(format!("coordination snapshot: {error}"));
            coordination::preview_coordination_snapshot()
        }
    };

    let runtime = match debug::fetch_runtime_debug_status().await {
        Ok(status) => status,
        Err(error) => {
            errors.push(format!("runtime debug: {error}"));
            debug::preview_runtime_debug_status()
        }
    };

    let server_debug = match debug::fetch_server_debug_summary().await {
        Ok(summary) => summary,
        Err(error) => {
            errors.push(format!("server debug summary: {error}"));
            debug::preview_server_debug_summary()
        }
    };

    BrowserBootstrapSnapshot {
        pwa,
        dev_auth_bootstrap,
        coordination,
        runtime,
        server_debug,
        errors,
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn collect_browser_bootstrap_snapshot() -> BrowserBootstrapSnapshot {
    BrowserBootstrapSnapshot {
        pwa: pwa::preview_pwa_environment(),
        dev_auth_bootstrap: auth::preview_dev_auth_bootstrap_status(),
        coordination: coordination::preview_coordination_snapshot(),
        runtime: debug::preview_runtime_debug_status(),
        server_debug: debug::preview_server_debug_summary(),
        errors: vec!["browser runtime unavailable".to_string()],
    }
}

#[must_use]
pub fn bootstrap_status_sections(
    snapshot: &BrowserBootstrapSnapshot,
) -> Vec<(&'static str, Vec<String>)> {
    let mut sections = vec![
        ("__SP42_PWA_STATUS__", pwa::pwa_status_lines(&snapshot.pwa)),
        (
            "__SP42_DEV_AUTH_BOOTSTRAP__",
            auth::bootstrap_status_lines(&snapshot.dev_auth_bootstrap),
        ),
        (
            "__SP42_COORDINATION_ROOMS__",
            coordination::coordination_snapshot_lines(&snapshot.coordination),
        ),
        (
            "__SP42_RUNTIME_STATUS__",
            debug::runtime_debug_status_lines(&snapshot.runtime),
        ),
        (
            "__SP42_SERVER_DEBUG__",
            debug::server_debug_summary_lines(&snapshot.server_debug),
        ),
    ];

    if !snapshot.errors.is_empty() {
        sections.push(("__SP42_BOOTSTRAP_ERRORS__", snapshot.errors.clone()));
    }

    sections
}

#[must_use]
pub fn bootstrap_error_lines(error: &str) -> Vec<String> {
    vec![format!("error={error}")]
}

#[cfg(test)]
mod tests {
    use super::{BrowserBootstrapSnapshot, bootstrap_error_lines, bootstrap_status_sections};
    use sp42_core::{
        CoordinationSnapshot, DevAuthCapabilityReport, DevAuthSessionStatus,
        LocalOAuthConfigStatus, ServerDebugSummary,
    };

    #[test]
    fn bootstrap_error_lines_prefix_error() {
        assert_eq!(
            bootstrap_error_lines("localhost unavailable"),
            vec!["error=localhost unavailable".to_string()]
        );
    }

    #[test]
    fn bootstrap_sections_include_error_section_when_present() {
        let snapshot = BrowserBootstrapSnapshot {
            pwa: super::pwa::PwaEnvironmentStatus {
                secure_context: true,
                online: true,
                service_worker_supported: true,
                service_worker_controlled: true,
                manifest_href: Some("/manifest.json".to_string()),
                registration_scope: Some("/".to_string()),
                waiting_worker: false,
                active_worker: true,
                active_cache: Some("sp42-shell-v5".to_string()),
                install_prompt_available: false,
                shell_mode: super::pwa::PwaShellMode::BrowserTab,
                browser_context: super::pwa::PwaBrowserContext {
                    display_mode_standalone: false,
                    ios_device: false,
                    browser_label: Some("Chromium".to_string()),
                },
                errors: vec![],
            },
            dev_auth_bootstrap: super::auth::DevAuthBootstrapStatus {
                bootstrap_ready: true,
                oauth: LocalOAuthConfigStatus::default(),
                session: DevAuthSessionStatus {
                    authenticated: false,
                    username: None,
                    scopes: vec![],
                    expires_at_ms: None,
                    token_present: false,
                    bridge_mode: "browser-preview".to_string(),
                    local_token_available: false,
                },
                source_path: None,
            },
            coordination: CoordinationSnapshot::default(),
            runtime: super::debug::RuntimeDebugStatus {
                project: "SP42".to_string(),
                uptime_ms: 1,
                auth: DevAuthSessionStatus {
                    authenticated: false,
                    username: None,
                    scopes: vec![],
                    expires_at_ms: None,
                    token_present: false,
                    bridge_mode: "browser-preview".to_string(),
                    local_token_available: false,
                },
                oauth: LocalOAuthConfigStatus::default(),
                bootstrap: super::auth::DevAuthBootstrapStatus {
                    bootstrap_ready: true,
                    oauth: LocalOAuthConfigStatus::default(),
                    session: DevAuthSessionStatus {
                        authenticated: false,
                        username: None,
                        scopes: vec![],
                        expires_at_ms: None,
                        token_present: false,
                        bridge_mode: "browser-preview".to_string(),
                        local_token_available: false,
                    },
                    source_path: None,
                },
                coordination: CoordinationSnapshot::default(),
            },
            server_debug: ServerDebugSummary {
                project: "SP42".to_string(),
                auth: DevAuthSessionStatus {
                    authenticated: false,
                    username: None,
                    scopes: vec![],
                    expires_at_ms: None,
                    token_present: false,
                    bridge_mode: "browser-preview".to_string(),
                    local_token_available: false,
                },
                oauth: LocalOAuthConfigStatus::default(),
                capabilities: DevAuthCapabilityReport {
                    checked: false,
                    wiki_id: "frwiki".to_string(),
                    username: None,
                    oauth_grants: vec![],
                    wiki_groups: vec![],
                    wiki_rights: vec![],
                    acceptance: DevAuthProbeAcceptance {
                        profile_accepted: false,
                        userinfo_accepted: false,
                    },
                    token_availability: DevAuthActionTokenAvailability {
                        csrf_token_available: false,
                        patrol_token_available: false,
                        rollback_token_available: false,
                    },
                    capabilities: sp42_core::DevAuthDerivedCapabilities::default(),
                    notes: vec![],
                    error: None,
                },
                coordination: CoordinationSnapshot::default(),
            },
            errors: vec!["runtime debug: localhost unavailable".to_string()],
        };

        let sections = bootstrap_status_sections(&snapshot);

        assert!(
            sections
                .iter()
                .any(|(key, _)| *key == "__SP42_PWA_STATUS__")
        );
        assert!(
            sections
                .iter()
                .any(|(key, _)| *key == "__SP42_BOOTSTRAP_ERRORS__")
        );
    }
}
