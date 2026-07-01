use axum::http::HeaderValue;
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeploymentMode {
    Local,
    Vps,
    Desktop,
}

impl DeploymentMode {
    pub(crate) fn from_env() -> Result<Self, String> {
        let raw = std::env::var("SP42_DEPLOYMENT_MODE").unwrap_or_else(|_| "local".to_string());
        match raw.trim() {
            "" | "local" => Ok(Self::Local),
            "vps" => Ok(Self::Vps),
            "desktop" => Ok(Self::Desktop),
            other => Err(format!(
                "SP42_DEPLOYMENT_MODE must be one of local, vps, desktop; got `{other}`"
            )),
        }
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Vps => "vps",
            Self::Desktop => "desktop",
        }
    }

    pub(crate) const fn permits_dev_token_bootstrap(self) -> bool {
        matches!(self, Self::Local)
    }

    pub(crate) const fn uses_secure_cookies(self) -> bool {
        matches!(self, Self::Vps)
    }

    /// The session cookie `SameSite` policy. Cross-site deployments (a different
    /// site frontend/API pair under `vps`, or the desktop `tauri://localhost` →
    /// loopback sidecar) need `None` so the browser sends the cookie on
    /// credentialed cross-site fetches; `Lax` is dropped on those. `local` is
    /// same-site (`localhost` across ports), so `Lax` is kept there. `None` is
    /// only honored by browsers alongside `Secure` (see `session_cookie_header`).
    /// Codex review #90.
    pub(crate) const fn session_cookie_same_site(self) -> &'static str {
        match self {
            Self::Local => "Lax",
            Self::Vps | Self::Desktop => "None",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeploymentConfig {
    pub mode: DeploymentMode,
    pub public_base_url: Option<String>,
    pub allowed_origins: Vec<HeaderValue>,
}

impl DeploymentConfig {
    pub(crate) fn load() -> Result<Self, String> {
        let mode = DeploymentMode::from_env()?;
        let public_base_url = public_base_url_from_env()?;
        let allowed_origins = allowed_origins_from_env(mode, public_base_url.as_deref())?;
        Ok(Self {
            mode,
            public_base_url,
            allowed_origins,
        })
    }
}

fn public_base_url_from_env() -> Result<Option<String>, String> {
    let Some(raw) = std::env::var("SP42_PUBLIC_BASE_URL")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };

    parse_http_url("SP42_PUBLIC_BASE_URL", &raw)?;
    Ok(Some(raw))
}

fn allowed_origins_from_env(
    mode: DeploymentMode,
    public_base_url: Option<&str>,
) -> Result<Vec<HeaderValue>, String> {
    let mut origins = default_origins_for_mode(mode);

    if let Some(public_base_url) = public_base_url {
        origins.push(origin_header_value(
            "SP42_PUBLIC_BASE_URL",
            public_base_url,
        )?);
    }

    if let Ok(raw) = std::env::var("SP42_ALLOWED_ORIGINS") {
        for origin in raw
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            origins.push(allowed_origin_header_value("SP42_ALLOWED_ORIGINS", origin)?);
        }
    }

    origins.sort();
    origins.dedup();
    Ok(origins)
}

fn default_origins_for_mode(mode: DeploymentMode) -> Vec<HeaderValue> {
    match mode {
        DeploymentMode::Local => LOCAL_ALLOWED_ORIGINS,
        DeploymentMode::Desktop => DESKTOP_ALLOWED_ORIGINS,
        DeploymentMode::Vps => &[],
    }
    .iter()
    .filter_map(|value| HeaderValue::from_str(value).ok())
    .collect()
}

fn header_value(value: &str) -> Result<HeaderValue, String> {
    HeaderValue::from_str(value)
        .map_err(|error| format!("invalid HTTP header value `{value}`: {error}"))
}

fn parse_http_url(label: &str, value: &str) -> Result<Url, String> {
    let url = Url::parse(value)
        .map_err(|error| format!("{label} contains invalid URL `{value}`: {error}"))?;
    if matches!(url.scheme(), "http" | "https") {
        Ok(url)
    } else {
        Err(format!("{label} must use http or https; got `{value}`"))
    }
}

fn origin_header_value(label: &str, value: &str) -> Result<HeaderValue, String> {
    let url = parse_http_url(label, value)?;
    header_value(&url.origin().ascii_serialization())
}

/// Canonicalize a configured allowed origin to `scheme://host[:port]`. Unlike
/// `origin_header_value` (for the public base URL, which must be HTTP(S)), this
/// accepts custom app schemes such as `tauri://localhost` so the desktop
/// sidecar's `SP42_ALLOWED_ORIGINS` loads instead of failing startup — the
/// default desktop allow list already trusts that origin, and it mirrors the
/// non-HTTP support in the redirect sanitizer. Codex review #90.
fn allowed_origin_header_value(label: &str, value: &str) -> Result<HeaderValue, String> {
    // Shared origin primitive: canonical scheme://host[:port], accepting custom
    // app schemes (e.g. tauri://localhost) so the desktop sidecar loads. ADR-0013.
    let origin = sp42_core::origin_of(value).ok_or_else(|| {
        format!("{label} origin `{value}` is not a valid scheme://host[:port] origin")
    })?;
    header_value(&origin)
}

const LOCAL_ALLOWED_ORIGINS: &[&str] = &[
    "http://127.0.0.1:4173",
    "http://localhost:4173",
    "http://127.0.0.1:8788",
    "http://localhost:8788",
];

const DESKTOP_ALLOWED_ORIGINS: &[&str] = &[
    "http://127.0.0.1:8788",
    "http://localhost:8788",
    "http://tauri.localhost",
    "https://tauri.localhost",
    "tauri://localhost",
];

#[cfg(test)]
mod tests {
    use super::{DeploymentMode, allowed_origin_header_value, allowed_origins_from_env};

    #[test]
    fn allowed_origin_accepts_custom_app_scheme() {
        // desktop sidecar sets SP42_ALLOWED_ORIGINS=tauri://localhost,...; the
        // env parser must accept that scheme or the desktop app fails to boot.
        // Codex review #90.
        let origin = allowed_origin_header_value("SP42_ALLOWED_ORIGINS", "tauri://localhost")
            .expect("custom app scheme should parse");
        assert_eq!(origin, "tauri://localhost");
        // http(s) still canonicalizes as before
        let http = allowed_origin_header_value("SP42_ALLOWED_ORIGINS", "http://tauri.localhost")
            .expect("http origin should parse");
        assert_eq!(http, "http://tauri.localhost");
        // authority-less values are still rejected
        assert!(allowed_origin_header_value("SP42_ALLOWED_ORIGINS", "not-a-url").is_err());
    }

    #[test]
    fn deployment_mode_labels_are_stable() {
        assert_eq!(DeploymentMode::Local.as_str(), "local");
        assert_eq!(DeploymentMode::Vps.as_str(), "vps");
        assert_eq!(DeploymentMode::Desktop.as_str(), "desktop");
    }

    #[test]
    fn local_mode_is_the_only_dev_token_mode() {
        assert!(DeploymentMode::Local.permits_dev_token_bootstrap());
        assert!(!DeploymentMode::Vps.permits_dev_token_bootstrap());
        assert!(!DeploymentMode::Desktop.permits_dev_token_bootstrap());
    }

    #[test]
    fn allowed_origins_include_public_base_url() {
        let origins =
            allowed_origins_from_env(DeploymentMode::Vps, Some("https://sp42.example.org"))
                .expect("origins should parse");
        assert!(
            origins
                .iter()
                .any(|origin| origin == "https://sp42.example.org")
        );
    }

    #[test]
    fn vps_mode_does_not_allow_localhost_origins_by_default() {
        let origins =
            allowed_origins_from_env(DeploymentMode::Vps, None).expect("origins should parse");
        assert!(origins.is_empty());
    }

    #[test]
    fn desktop_mode_allows_tauri_app_origins_by_default() {
        let origins =
            allowed_origins_from_env(DeploymentMode::Desktop, None).expect("origins should parse");
        assert!(
            origins
                .iter()
                .any(|origin| origin == "http://tauri.localhost")
        );
        assert!(origins.iter().any(|origin| origin == "tauri://localhost"));
    }
}
