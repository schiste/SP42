use std::env;
use std::error::Error;
use std::io;
use std::path::PathBuf;
use std::sync::Mutex;

use sp42_core::branding::{PROJECT_NAME, USER_AGENT};
use tauri::webview::WebviewWindowBuilder;
use tauri::{App, AppHandle, Manager, RunEvent, WebviewUrl};
use tauri_plugin_shell::ShellExt;
use tauri_plugin_shell::process::{CommandChild, CommandEvent};

const DEFAULT_SIDECAR_BIND_ADDR: &str = "127.0.0.1:8788";
const SIDECAR_COMMAND: &str = "sp42-server";

type SetupResult<T> = Result<T, Box<dyn Error>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DesktopBackendMode {
    Sidecar,
    Remote,
}

impl DesktopBackendMode {
    fn from_env() -> Result<Self, String> {
        let raw = env::var("SP42_DESKTOP_BACKEND_MODE")
            .or_else(|_| env::var("SP42_DESKTOP_BACKEND"))
            .unwrap_or_else(|_| "sidecar".to_string());

        match raw.trim() {
            "" | "sidecar" | "local" => Ok(Self::Sidecar),
            "remote" | "vps" => Ok(Self::Remote),
            other => Err(format!(
                "SP42_DESKTOP_BACKEND_MODE must be sidecar or remote; got `{other}`"
            )),
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Sidecar => "sidecar",
            Self::Remote => "remote",
        }
    }
}

#[derive(Debug, Clone)]
struct DesktopBackendConfig {
    mode: DesktopBackendMode,
    api_base_url: String,
    default_wiki_id: String,
    sidecar_bind_addr: String,
    runtime_dir: PathBuf,
}

impl DesktopBackendConfig {
    fn load(app: &AppHandle) -> SetupResult<Self> {
        let mode = DesktopBackendMode::from_env().map_err(setup_error)?;
        let sidecar_bind_addr = env::var("SP42_DESKTOP_SIDECAR_BIND_ADDR")
            .or_else(|_| env::var("SP42_BIND_ADDR"))
            .unwrap_or_else(|_| DEFAULT_SIDECAR_BIND_ADDR.to_string());
        let runtime_dir = match env::var_os("SP42_RUNTIME_DIR") {
            Some(path) => PathBuf::from(path),
            None => app.path().app_data_dir()?.join("runtime"),
        };
        let api_base_url = match mode {
            DesktopBackendMode::Sidecar => env::var("SP42_DESKTOP_BACKEND_URL")
                .unwrap_or_else(|_| format!("http://{sidecar_bind_addr}")),
            DesktopBackendMode::Remote => env::var("SP42_DESKTOP_REMOTE_BACKEND_URL")
                .or_else(|_| env::var("SP42_PUBLIC_BASE_URL"))
                .map_err(|_| {
                    setup_error(
                        "remote desktop mode requires SP42_DESKTOP_REMOTE_BACKEND_URL or SP42_PUBLIC_BASE_URL",
                    )
                })?,
        };
        let default_wiki_id = env::var("SP42_DEFAULT_WIKI_ID")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "frwiki".to_string());

        Ok(Self {
            mode,
            api_base_url: normalize_base_url(&api_base_url),
            default_wiki_id,
            sidecar_bind_addr,
            runtime_dir,
        })
    }
}

#[derive(Default)]
struct SidecarState {
    child: Mutex<Option<CommandChild>>,
}

pub fn run() -> tauri::Result<()> {
    let context = tauri::generate_context!();
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(SidecarState::default())
        .setup(|app| {
            let config = DesktopBackendConfig::load(app.handle())?;
            if config.mode == DesktopBackendMode::Sidecar {
                start_sidecar(app.handle(), &config)?;
            }
            create_main_window(app, &config)?;
            Ok(())
        })
        .build(context)?;

    app.run(|app_handle, event| {
        if matches!(event, RunEvent::ExitRequested { .. } | RunEvent::Exit) {
            stop_sidecar(app_handle);
        }
    });
    Ok(())
}

fn start_sidecar(app: &AppHandle, config: &DesktopBackendConfig) -> SetupResult<()> {
    std::fs::create_dir_all(&config.runtime_dir)?;

    let command = app
        .shell()
        .sidecar(SIDECAR_COMMAND)?
        .env("SP42_DEPLOYMENT_MODE", "desktop")
        .env("SP42_BIND_ADDR", &config.sidecar_bind_addr)
        .env("SP42_PUBLIC_BASE_URL", &config.api_base_url)
        .env("SP42_RUNTIME_DIR", &config.runtime_dir)
        .env(
            "SP42_ALLOWED_ORIGINS",
            "tauri://localhost,http://tauri.localhost,https://tauri.localhost",
        );
    let (mut rx, child) = command.spawn()?;

    app.state::<SidecarState>()
        .child
        .lock()
        .map_err(|_| setup_error("desktop sidecar state lock is poisoned"))?
        .replace(child);

    tauri::async_runtime::spawn(async move {
        while let Some(event) = rx.recv().await {
            match event {
                CommandEvent::Stdout(bytes) => {
                    eprintln!(
                        "[sp42-server] {}",
                        String::from_utf8_lossy(&bytes).trim_end()
                    );
                }
                CommandEvent::Stderr(bytes) => {
                    eprintln!(
                        "[sp42-server] {}",
                        String::from_utf8_lossy(&bytes).trim_end()
                    );
                }
                CommandEvent::Terminated(status) => {
                    eprintln!("[sp42-server] sidecar terminated: {status:?}");
                }
                _ => {}
            }
        }
    });

    Ok(())
}

fn stop_sidecar(app: &AppHandle) {
    let state = app.state::<SidecarState>();
    if let Ok(mut child) = state.child.lock()
        && let Some(child) = child.take()
    {
        let _ = child.kill();
    }
}

fn create_main_window(app: &App, config: &DesktopBackendConfig) -> SetupResult<()> {
    WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title(PROJECT_NAME)
        .inner_size(1440.0, 960.0)
        .min_inner_size(960.0, 720.0)
        .user_agent(USER_AGENT)
        .initialization_script(runtime_config_script(config)?)
        .build()?;
    Ok(())
}

fn runtime_config_script(config: &DesktopBackendConfig) -> SetupResult<String> {
    let api_base_url = serde_json::to_string(&config.api_base_url)?;
    let default_wiki_id = serde_json::to_string(&config.default_wiki_id)?;
    let backend_mode = serde_json::to_string(config.mode.as_str())?;
    Ok(format!(
        r#"
(() => {{
  const current = window.__SP42_RUNTIME_CONFIG__ || {{}};
  window.__SP42_RUNTIME_CONFIG__ = {{
    ...current,
    apiBaseUrl: {api_base_url},
    defaultWikiId: {default_wiki_id},
    deploymentMode: "desktop",
    desktopBackendMode: {backend_mode}
  }};
}})();
"#
    ))
}

fn normalize_base_url(value: &str) -> String {
    value.trim().trim_end_matches('/').to_string()
}

fn setup_error(message: impl Into<String>) -> Box<dyn Error> {
    Box::new(io::Error::other(message.into()))
}

#[cfg(test)]
mod tests {
    use super::{DesktopBackendMode, SIDECAR_COMMAND, normalize_base_url};

    #[test]
    fn backend_mode_labels_are_stable() {
        assert_eq!(DesktopBackendMode::Sidecar.as_str(), "sidecar");
        assert_eq!(DesktopBackendMode::Remote.as_str(), "remote");
    }

    #[test]
    fn normalizes_api_base_url() {
        assert_eq!(
            normalize_base_url(" https://sp42.example.org/// "),
            "https://sp42.example.org"
        );
    }

    #[test]
    fn sidecar_command_matches_packaged_executable_name() {
        assert_eq!(SIDECAR_COMMAND, "sp42-server");
        assert!(!SIDECAR_COMMAND.contains('/'));
        assert!(!SIDECAR_COMMAND.contains('\\'));
    }
}
