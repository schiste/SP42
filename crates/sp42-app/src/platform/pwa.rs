//! PWA registration, manifest injection, install prompt handling, and status
//! reporting for the browser target.
//!
//! The service worker file (`sw.js`) and manifest (`manifest.json`) live in
//! `crates/sp42-app/static/` and must be served from the web root.

#[cfg(target_arch = "wasm32")]
use std::cell::{Cell, RefCell};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsCast;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsValue;

#[cfg(target_arch = "wasm32")]
use super::globals;

const MANIFEST_PATH: &str = "/manifest.json";
const SW_PATH: &str = "/sw.js";
const OFFLINE_PATH: &str = "/offline.html";
const ICON_192_PATH: &str = "/icons/sp42-icon-192.svg";
const ICON_512_PATH: &str = "/icons/sp42-icon-512.svg";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PwaShellMode {
    Unsupported,
    BrowserTab,
    InstalledStandalone,
    IosStandalone,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PwaBrowserContext {
    pub display_mode_standalone: bool,
    pub ios_device: bool,
    pub browser_label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PwaEnvironmentStatus {
    pub secure_context: bool,
    pub online: bool,
    pub service_worker_supported: bool,
    pub service_worker_controlled: bool,
    pub manifest_href: Option<String>,
    pub registration_scope: Option<String>,
    pub waiting_worker: bool,
    pub active_worker: bool,
    pub active_cache: Option<String>,
    pub install_prompt_available: bool,
    pub shell_mode: PwaShellMode,
    pub browser_context: PwaBrowserContext,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceWorkerRegistrationStatus {
    pub scope: String,
    pub waiting_worker: bool,
    pub active_worker: bool,
    pub installing_worker: bool,
}

#[must_use]
pub fn is_probably_secure_origin(protocol: &str, hostname: &str) -> bool {
    protocol == "https:" || hostname == "localhost" || hostname == "127.0.0.1"
}

#[must_use]
pub fn normalize_manifest_href(href: Option<&str>) -> Option<String> {
    href.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

#[must_use]
pub fn is_ios_user_agent(user_agent: &str) -> bool {
    let ua = user_agent.trim();
    !ua.is_empty() && (ua.contains("iPhone") || ua.contains("iPad") || ua.contains("iPod"))
}

#[must_use]
pub fn browser_label_from_user_agent(user_agent: Option<&str>) -> Option<String> {
    let ua = user_agent?.trim();
    if ua.is_empty() {
        return None;
    }

    let label = if is_ios_user_agent(ua) {
        "iOS Safari"
    } else if ua.contains("Edg/") {
        "Edge"
    } else if ua.contains("Chrome/") {
        "Chromium"
    } else if ua.contains("Firefox/") {
        "Firefox"
    } else if ua.contains("Safari/") {
        "Safari"
    } else {
        "Unknown browser"
    };

    Some(label.to_string())
}

#[must_use]
pub fn classify_shell_mode(status: &PwaEnvironmentStatus) -> PwaShellMode {
    if !status.secure_context && !status.service_worker_supported && status.manifest_href.is_none()
    {
        return PwaShellMode::Unsupported;
    }

    if status.browser_context.display_mode_standalone {
        if status.browser_context.ios_device {
            PwaShellMode::IosStandalone
        } else {
            PwaShellMode::InstalledStandalone
        }
    } else {
        PwaShellMode::BrowserTab
    }
}

#[must_use]
pub fn shell_mode_label(mode: PwaShellMode) -> &'static str {
    match mode {
        PwaShellMode::Unsupported => "Unsupported",
        PwaShellMode::BrowserTab => "Browser tab",
        PwaShellMode::InstalledStandalone => "Installed standalone",
        PwaShellMode::IosStandalone => "iOS standalone",
    }
}

#[must_use]
pub fn install_state_label(status: &PwaEnvironmentStatus) -> &'static str {
    if is_install_ready(status) {
        if status.install_prompt_available {
            "Install ready"
        } else {
            "Install ready, prompt pending"
        }
    } else if status.service_worker_supported && status.secure_context {
        "Install limited"
    } else {
        "Install unavailable"
    }
}

#[must_use]
pub fn update_state_label(status: &PwaEnvironmentStatus) -> &'static str {
    if is_update_ready(status) {
        "Updates ready"
    } else if status.service_worker_supported && status.secure_context {
        "Updates limited"
    } else {
        "Updates unavailable"
    }
}

#[must_use]
pub fn offline_state_label(status: &PwaEnvironmentStatus) -> &'static str {
    if !status.online && status.secure_context && status.service_worker_supported {
        "Offline shell active"
    } else if status.secure_context
        && status.service_worker_supported
        && status.registration_scope.is_some()
    {
        "Offline shell ready"
    } else if status.secure_context && status.service_worker_supported {
        "Offline shell registering"
    } else {
        "Offline shell unavailable"
    }
}

#[must_use]
pub fn pwa_guidance_lines(status: &PwaEnvironmentStatus) -> Vec<String> {
    let mut lines = Vec::new();

    match status.shell_mode {
        PwaShellMode::Unsupported => {
            lines.push("PWA helpers are unavailable outside a browser context.".to_string());
        }
        PwaShellMode::BrowserTab => {
            if status.install_prompt_available {
                lines.push(
                    "Your browser surfaced an install prompt; accept it to install SP42."
                        .to_string(),
                );
            } else if status.browser_context.ios_device {
                lines.push(
                    "Install prompt is not expected on iPhone or iPad; use Share -> Add to Home Screen."
                        .to_string(),
                );
            } else {
                lines.push(
                    "Install prompt is not currently available; browser support and site readiness can surface it later."
                        .to_string(),
                );
            }
        }
        PwaShellMode::InstalledStandalone => {
            lines.push("This is the installed standalone shell; refresh to pick up service-worker updates.".to_string());
        }
        PwaShellMode::IosStandalone => {
            lines.push("iOS standalone mode detected; launch from the Home Screen for the installed shell.".to_string());
        }
    }

    if status.browser_context.ios_device && !status.browser_context.display_mode_standalone {
        lines.push(
            "On iPhone or iPad, install via Share -> Add to Home Screen because beforeinstallprompt is not reliable."
                .to_string(),
        );
    }

    if status.secure_context && status.service_worker_supported {
        if status.registration_scope.is_some() {
            lines.push(
                "Service worker registration is active; offline shell and update checks are enabled."
                    .to_string(),
            );
            if status.waiting_worker {
                lines.push(
                    "A waiting service worker is ready; refresh this page or activate the update to claim the new shell."
                        .to_string(),
                );
            } else {
                lines.push(
                    "When you ship a new shell, refresh this page or reopen the installed app to pick up the update."
                        .to_string(),
                );
            }
        } else {
            lines.push(
                "Secure context is present, but the service worker has not registered yet."
                    .to_string(),
            );
        }
    } else {
        lines.push(
            "Offline behavior remains limited until a secure context and service worker support are available."
                .to_string(),
        );
    }

    if status.manifest_href.is_none() {
        lines.push(
            "The manifest link is not yet declared, so installability is incomplete.".to_string(),
        );
    }

    if !status.online {
        lines.push(
            "The browser currently reports an offline network state; live patrol actions stay suspended until reconnect."
                .to_string(),
        );
    }

    if status.service_worker_supported && !status.service_worker_controlled {
        lines.push(
            "The service worker is not controlling this tab yet; reload once after registration for full offline coverage."
                .to_string(),
        );
    }

    lines
}

#[must_use]
pub fn pwa_status_lines(status: &PwaEnvironmentStatus) -> Vec<String> {
    let mut lines = vec![
        format!("secure_context={}", status.secure_context),
        format!("online={}", status.online),
        format!(
            "service_worker_supported={}",
            status.service_worker_supported
        ),
        format!(
            "service_worker_controlled={}",
            status.service_worker_controlled
        ),
        format!("shell_mode={}", shell_mode_label(status.shell_mode)),
        format!(
            "browser_context_standalone={}",
            status.browser_context.display_mode_standalone
        ),
        format!("browser_context_ios={}", status.browser_context.ios_device),
        format!(
            "browser_label={}",
            status
                .browser_context
                .browser_label
                .as_deref()
                .unwrap_or("not detected")
        ),
        format!(
            "registration_scope={}",
            status
                .registration_scope
                .as_deref()
                .unwrap_or("not registered")
        ),
        format!("waiting_worker={}", status.waiting_worker),
        format!("active_worker={}", status.active_worker),
        format!(
            "active_cache={}",
            status.active_cache.as_deref().unwrap_or("not reported")
        ),
        format!(
            "manifest_href={}",
            status.manifest_href.as_deref().unwrap_or("not declared")
        ),
        format!(
            "install_prompt_available={}",
            status.install_prompt_available
        ),
        format!("install_ready={}", is_install_ready(status)),
        format!("update_ready={}", is_update_ready(status)),
        format!("install_state={}", install_state_label(status)),
        format!("update_state={}", update_state_label(status)),
        format!("offline_state={}", offline_state_label(status)),
        format!("errors={}", status.errors.len()),
    ];

    lines.extend(
        pwa_guidance_lines(status)
            .into_iter()
            .map(|line| format!("guidance={line}")),
    );

    for error in &status.errors {
        lines.push(format!("  error: {error}"));
    }

    lines
}

#[must_use]
pub fn manifest_path() -> &'static str {
    MANIFEST_PATH
}

#[must_use]
pub fn service_worker_path() -> &'static str {
    SW_PATH
}

#[must_use]
pub fn offline_fallback_path() -> &'static str {
    OFFLINE_PATH
}

#[must_use]
pub fn icon_192_path() -> &'static str {
    ICON_192_PATH
}

#[must_use]
pub fn icon_512_path() -> &'static str {
    ICON_512_PATH
}

#[must_use]
pub fn shell_asset_paths() -> [&'static str; 5] {
    [
        manifest_path(),
        service_worker_path(),
        offline_fallback_path(),
        icon_192_path(),
        icon_512_path(),
    ]
}

#[cfg(target_arch = "wasm32")]
#[must_use]
pub fn is_localhost_origin() -> bool {
    let Some(window) = globals::browser_window() else {
        return false;
    };

    matches!(
        window.location().hostname().ok().as_deref(),
        Some("127.0.0.1" | "localhost")
    )
}

#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn is_localhost_origin() -> bool {
    false
}

#[must_use]
pub fn is_install_ready(status: &PwaEnvironmentStatus) -> bool {
    status.secure_context
        && status.online
        && status.service_worker_supported
        && status.manifest_href.is_some()
        && status.registration_scope.is_some()
}

#[must_use]
pub fn is_update_ready(status: &PwaEnvironmentStatus) -> bool {
    status.secure_context
        && status.service_worker_supported
        && status.registration_scope.is_some()
        && status.waiting_worker
}

#[must_use]
pub fn preview_pwa_environment() -> PwaEnvironmentStatus {
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(window) = globals::browser_window() {
            let secure_context = window.is_secure_context();
            let manifest_href = manifest_link_href(globals::browser_document().as_ref());
            let browser_context = preview_pwa_browser_context();

            let mut status = PwaEnvironmentStatus {
                secure_context,
                online: browser_online(),
                service_worker_supported: globals::browser_service_worker_container().is_some(),
                service_worker_controlled: browser_service_worker_controlled(),
                manifest_href,
                registration_scope: None,
                waiting_worker: service_worker_update_ready(),
                active_worker: false,
                active_cache: service_worker_active_cache(),
                install_prompt_available: false,
                shell_mode: PwaShellMode::BrowserTab,
                browser_context,
                errors: Vec::new(),
            };
            status.shell_mode = classify_shell_mode(&status);
            return status;
        }
    }

    PwaEnvironmentStatus {
        secure_context: false,
        online: false,
        service_worker_supported: false,
        service_worker_controlled: false,
        manifest_href: None,
        registration_scope: None,
        waiting_worker: false,
        active_worker: false,
        active_cache: None,
        install_prompt_available: false,
        shell_mode: PwaShellMode::Unsupported,
        browser_context: PwaBrowserContext::default(),
        errors: Vec::new(),
    }
}

#[cfg(target_arch = "wasm32")]
#[must_use]
pub fn preview_pwa_browser_context() -> PwaBrowserContext {
    let user_agent = browser_user_agent();
    let display_mode_standalone =
        browser_display_mode_standalone() || browser_ios_standalone_hint();
    let browser_label = browser_label_from_user_agent(user_agent.as_deref());
    let ios_device = user_agent.as_deref().is_some_and(is_ios_user_agent);

    PwaBrowserContext {
        display_mode_standalone,
        ios_device,
        browser_label,
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn preview_pwa_browser_context() -> PwaBrowserContext {
    PwaBrowserContext::default()
}

#[cfg(target_arch = "wasm32")]
fn browser_display_mode_standalone() -> bool {
    let Some(window) = globals::browser_window() else {
        return false;
    };

    let Ok(matcher) = js_sys::Reflect::get(window.as_ref(), &JsValue::from_str("matchMedia"))
    else {
        return false;
    };
    let Ok(matcher) = matcher.dyn_into::<js_sys::Function>() else {
        return false;
    };
    let Ok(query) = matcher.call1(
        window.as_ref(),
        &JsValue::from_str("(display-mode: standalone)"),
    ) else {
        return false;
    };

    js_sys::Reflect::get(&query, &JsValue::from_str("matches"))
        .ok()
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

#[cfg(target_arch = "wasm32")]
fn browser_ios_standalone_hint() -> bool {
    let Some(navigator) = globals::browser_navigator() else {
        return false;
    };

    js_sys::Reflect::get(navigator.as_ref(), &JsValue::from_str("standalone"))
        .ok()
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

#[cfg(target_arch = "wasm32")]
fn browser_user_agent() -> Option<String> {
    globals::browser_navigator()
        .and_then(|navigator| navigator.user_agent().ok())
        .and_then(|value| {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        })
}

#[cfg(not(target_arch = "wasm32"))]
fn browser_display_mode_standalone() -> bool {
    false
}

#[cfg(not(target_arch = "wasm32"))]
fn browser_ios_standalone_hint() -> bool {
    false
}

#[cfg(not(target_arch = "wasm32"))]
fn browser_user_agent() -> Option<String> {
    None
}

#[cfg(target_arch = "wasm32")]
fn browser_online() -> bool {
    globals::browser_navigator()
        .and_then(|navigator| {
            js_sys::Reflect::get(navigator.as_ref(), &JsValue::from_str("onLine")).ok()
        })
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

#[cfg(not(target_arch = "wasm32"))]
fn browser_online() -> bool {
    false
}

#[cfg(target_arch = "wasm32")]
fn browser_service_worker_controlled() -> bool {
    globals::browser_service_worker_container()
        .is_some_and(|container| container.controller().is_some())
}

#[cfg(not(target_arch = "wasm32"))]
fn browser_service_worker_controlled() -> bool {
    false
}

#[cfg(target_arch = "wasm32")]
fn service_worker_update_ready() -> bool {
    SERVICE_WORKER_UPDATE_READY.with(|cell| cell.get())
}

#[cfg(not(target_arch = "wasm32"))]
fn service_worker_update_ready() -> bool {
    false
}

#[cfg(target_arch = "wasm32")]
fn service_worker_active_cache() -> Option<String> {
    SERVICE_WORKER_ACTIVE_CACHE.with(|cell| cell.borrow().clone())
}

#[cfg(not(target_arch = "wasm32"))]
fn service_worker_active_cache() -> Option<String> {
    None
}

// ---------------------------------------------------------------------------
// Thread-local storage for the deferred install prompt (Chromium only).
// Follows the same pattern as EVENT_SOURCE_HANDLES in runtime.rs.
// ---------------------------------------------------------------------------

#[cfg(target_arch = "wasm32")]
thread_local! {
    static DEFERRED_INSTALL_PROMPT: RefCell<Option<JsValue>> = const {
        RefCell::new(None)
    };
    static INSTALL_PROMPT_LISTENER_ATTACHED: Cell<bool> = const { Cell::new(false) };
    static SERVICE_WORKER_MESSAGE_LISTENER_ATTACHED: Cell<bool> = const { Cell::new(false) };
    static SERVICE_WORKER_UPDATE_READY: Cell<bool> = const { Cell::new(false) };
    static SERVICE_WORKER_ACTIVE_CACHE: RefCell<Option<String>> = const { RefCell::new(None) };
}

// ---------------------------------------------------------------------------
// Manifest link injection
// ---------------------------------------------------------------------------

/// Inject a `<link rel="manifest">` element into the document `<head>`.
///
/// Returns `true` if the element was appended successfully.
#[cfg(target_arch = "wasm32")]
pub fn inject_manifest_link() -> bool {
    let Some(document) = globals::browser_document() else {
        return false;
    };
    let Some(head) = document.head() else {
        return false;
    };

    // Do not inject a second manifest link if one already exists.
    if manifest_link_href(Some(&document)).is_some() {
        return true;
    }

    let Ok(link) = document.create_element("link") else {
        return false;
    };
    let _ = link.set_attribute("rel", "manifest");
    let _ = link.set_attribute("href", MANIFEST_PATH);

    head.append_child(&link).is_ok()
}

#[cfg(not(target_arch = "wasm32"))]
pub fn inject_manifest_link() -> bool {
    false
}

// ---------------------------------------------------------------------------
// Install prompt
// ---------------------------------------------------------------------------

/// Attach a listener for the `beforeinstallprompt` event (Chromium browsers).
///
/// The event is stored in thread-local storage for later use by
/// [`trigger_install_prompt`].  Returns `true` if the listener was attached.
#[cfg(target_arch = "wasm32")]
pub fn listen_for_install_prompt() -> bool {
    use wasm_bindgen::closure::Closure;

    // Guard: only attach the listener once per page lifetime.
    if INSTALL_PROMPT_LISTENER_ATTACHED.with(|flag| flag.get()) {
        return true;
    }

    let Some(_window) = globals::browser_window() else {
        return false;
    };

    let callback = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
        event.prevent_default();
        let prompt_event: JsValue = event.into();
        DEFERRED_INSTALL_PROMPT.with(|cell| {
            *cell.borrow_mut() = Some(prompt_event);
        });
    });

    let success = _window
        .add_event_listener_with_callback("beforeinstallprompt", callback.as_ref().unchecked_ref())
        .is_ok();

    // Leak the closure intentionally — it must live for the page lifetime.
    // There is no struct to hold it (unlike EventSource/WebSocket handles).
    callback.forget();

    if success {
        INSTALL_PROMPT_LISTENER_ATTACHED.with(|flag| flag.set(true));
    }

    success
}

#[cfg(not(target_arch = "wasm32"))]
pub fn listen_for_install_prompt() -> bool {
    false
}

#[cfg(target_arch = "wasm32")]
pub fn listen_for_service_worker_messages() -> bool {
    use wasm_bindgen::closure::Closure;

    if SERVICE_WORKER_MESSAGE_LISTENER_ATTACHED.with(|flag| flag.get()) {
        return true;
    }

    let Some(container) = globals::browser_service_worker_container() else {
        return false;
    };

    let callback =
        Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |event: web_sys::MessageEvent| {
            let data = event.data();
            let event_type = js_sys::Reflect::get(&data, &JsValue::from_str("type"))
                .ok()
                .and_then(|value| value.as_string());
            let cache_name = js_sys::Reflect::get(&data, &JsValue::from_str("cache"))
                .ok()
                .and_then(|value| value.as_string());

            if let Some(cache_name) = cache_name {
                SERVICE_WORKER_ACTIVE_CACHE.with(|cell| {
                    *cell.borrow_mut() = Some(cache_name);
                });
            }

            match event_type.as_deref() {
                Some("SP42_SW_UPDATE_READY") => {
                    SERVICE_WORKER_UPDATE_READY.with(|cell| cell.set(true));
                }
                Some("SP42_SW_ACTIVE") => {
                    SERVICE_WORKER_UPDATE_READY.with(|cell| cell.set(false));
                }
                _ => {}
            }
        });

    let success = container
        .add_event_listener_with_callback("message", callback.as_ref().unchecked_ref())
        .is_ok();
    callback.forget();

    if success {
        SERVICE_WORKER_MESSAGE_LISTENER_ATTACHED.with(|flag| flag.set(true));
    }

    success
}

#[cfg(not(target_arch = "wasm32"))]
pub fn listen_for_service_worker_messages() -> bool {
    false
}

/// Fire the deferred install prompt.
///
/// Returns `Ok(true)` if the user accepted, `Ok(false)` if dismissed.
/// The prompt is consumed — subsequent calls return `Err` until the browser
/// fires a new `beforeinstallprompt`.
#[cfg(target_arch = "wasm32")]
pub async fn trigger_install_prompt() -> Result<bool, String> {
    use super::js_error_message;

    let prompt = DEFERRED_INSTALL_PROMPT.with(|cell| cell.borrow_mut().take());

    let Some(prompt) = prompt else {
        return Err("No install prompt available.".to_string());
    };

    let prompt_fn = js_sys::Reflect::get(&prompt, &"prompt".into()).map_err(js_error_message)?;
    let prompt_fn: js_sys::Function = prompt_fn
        .dyn_into()
        .map_err(|_| "prompt is not a function".to_string())?;

    let promise: js_sys::Promise = prompt_fn
        .call0(&prompt)
        .map_err(js_error_message)?
        .dyn_into()
        .map_err(|_| "prompt() did not return a Promise".to_string())?;

    let result = wasm_bindgen_futures::JsFuture::from(promise)
        .await
        .map_err(js_error_message)?;

    let outcome = js_sys::Reflect::get(&result, &"outcome".into())
        .ok()
        .and_then(|value| value.as_string())
        .unwrap_or_else(|| "unknown".to_string());

    Ok(outcome == "accepted")
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn trigger_install_prompt() -> Result<bool, String> {
    Err("Install prompt is only available in the browser runtime.".to_string())
}

/// Check whether a deferred install prompt is currently stored.
#[cfg(target_arch = "wasm32")]
#[must_use]
pub fn is_install_prompt_available() -> bool {
    DEFERRED_INSTALL_PROMPT.with(|cell| cell.borrow().is_some())
}

#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn is_install_prompt_available() -> bool {
    false
}

// ---------------------------------------------------------------------------
// Full PWA initialization (convenience wrapper)
// ---------------------------------------------------------------------------

/// Run the full PWA initialization sequence: inject manifest link, listen for
/// install prompt, and register the service worker.  Returns a status summary.
#[cfg(target_arch = "wasm32")]
pub async fn initialize_pwa() -> PwaEnvironmentStatus {
    let manifest_injected = inject_manifest_link();
    let listener_attached = listen_for_install_prompt();
    let sw_listener_attached = listen_for_service_worker_messages();
    let localhost_origin = is_localhost_origin();

    let mut errors = Vec::new();
    if !manifest_injected {
        errors.push("manifest_link: failed to inject".to_string());
    }
    if !listener_attached {
        errors.push("install_prompt_listener: failed to attach".to_string());
    }
    if !sw_listener_attached {
        errors.push("sw_message_listener: failed to attach".to_string());
    }
    let registration = if localhost_origin {
        errors.push(
            "sw_registration: skipped on localhost to avoid stale dev shell caching".to_string(),
        );
        None
    } else {
        match register_service_worker(service_worker_path()).await {
            Ok(reg) => Some(reg),
            Err(error) => {
                errors.push(format!("sw_registration: {error}"));
                None
            }
        }
    };

    let mut status = preview_pwa_environment();
    if manifest_injected {
        status.manifest_href = Some(manifest_path().to_string());
    }
    if let Some(registration) = registration {
        status.registration_scope = Some(registration.scope);
        status.waiting_worker = registration.waiting_worker;
        status.active_worker = registration.active_worker;
    }
    status.install_prompt_available = is_install_prompt_available();
    status.shell_mode = classify_shell_mode(&status);
    status.errors = errors;
    status
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn initialize_pwa() -> PwaEnvironmentStatus {
    preview_pwa_environment()
}

#[cfg(target_arch = "wasm32")]
pub async fn fetch_pwa_environment_status() -> Result<PwaEnvironmentStatus, String> {
    let secure_context = globals::browser_is_secure_context();
    let manifest_href = manifest_link_href(globals::browser_document().as_ref());
    let browser_context = preview_pwa_browser_context();
    let registration = fetch_service_worker_registration().await?;

    let mut status = PwaEnvironmentStatus {
        secure_context,
        online: browser_online(),
        service_worker_supported: globals::browser_service_worker_container().is_some(),
        service_worker_controlled: browser_service_worker_controlled(),
        manifest_href,
        registration_scope: registration
            .as_ref()
            .map(|registration| registration.scope.clone()),
        waiting_worker: registration
            .as_ref()
            .is_some_and(|registration| registration.waiting_worker)
            || service_worker_update_ready(),
        active_worker: registration
            .as_ref()
            .is_some_and(|registration| registration.active_worker),
        active_cache: service_worker_active_cache(),
        install_prompt_available: is_install_prompt_available(),
        shell_mode: PwaShellMode::BrowserTab,
        browser_context,
        errors: Vec::new(),
    };
    status.shell_mode = classify_shell_mode(&status);
    Ok(status)
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn fetch_pwa_environment_status() -> Result<PwaEnvironmentStatus, String> {
    Ok(preview_pwa_environment())
}

#[cfg(target_arch = "wasm32")]
pub async fn register_service_worker(
    script_url: &str,
) -> Result<ServiceWorkerRegistrationStatus, String> {
    let Some(service_worker_container) = globals::browser_service_worker_container() else {
        return Err("service workers are unavailable in this browser context".to_string());
    };

    let promise = service_worker_container.register(script_url);
    let registration = wasm_bindgen_futures::JsFuture::from(promise)
        .await
        .map_err(super::js_error_message)?;
    let registration: web_sys::ServiceWorkerRegistration =
        registration.dyn_into().map_err(super::js_error_message)?;

    Ok(service_worker_registration_status(&registration))
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn register_service_worker(
    _script_url: &str,
) -> Result<ServiceWorkerRegistrationStatus, String> {
    Err("service worker registration is only available in the browser runtime".to_string())
}

#[cfg(target_arch = "wasm32")]
pub async fn fetch_service_worker_registration()
-> Result<Option<ServiceWorkerRegistrationStatus>, String> {
    let Some(service_worker_container) = globals::browser_service_worker_container() else {
        return Ok(None);
    };

    let registration =
        wasm_bindgen_futures::JsFuture::from(service_worker_container.get_registration())
            .await
            .map_err(super::js_error_message)?;

    if registration.is_undefined() || registration.is_null() {
        return Ok(None);
    }

    let registration: web_sys::ServiceWorkerRegistration =
        registration.dyn_into().map_err(super::js_error_message)?;
    Ok(Some(service_worker_registration_status(&registration)))
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn fetch_service_worker_registration()
-> Result<Option<ServiceWorkerRegistrationStatus>, String> {
    Ok(None)
}

#[cfg(target_arch = "wasm32")]
pub async fn activate_waiting_service_worker() -> Result<bool, String> {
    let Some(service_worker_container) = globals::browser_service_worker_container() else {
        return Err("service workers are unavailable in this browser context".to_string());
    };

    let registration =
        wasm_bindgen_futures::JsFuture::from(service_worker_container.get_registration())
            .await
            .map_err(super::js_error_message)?;

    if registration.is_undefined() || registration.is_null() {
        return Ok(false);
    }

    let registration: web_sys::ServiceWorkerRegistration =
        registration.dyn_into().map_err(super::js_error_message)?;
    let Some(worker) = registration.waiting() else {
        return Ok(false);
    };

    let message = js_sys::Object::new();
    js_sys::Reflect::set(
        message.as_ref(),
        &JsValue::from_str("type"),
        &JsValue::from_str("SKIP_WAITING"),
    )
    .map_err(super::js_error_message)?;

    worker
        .post_message(message.as_ref())
        .map_err(super::js_error_message)?;

    Ok(true)
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn activate_waiting_service_worker() -> Result<bool, String> {
    Ok(false)
}

#[cfg(target_arch = "wasm32")]
fn manifest_link_href(document: Option<&web_sys::Document>) -> Option<String> {
    document
        .and_then(|document| {
            document
                .query_selector("link[rel='manifest']")
                .ok()
                .flatten()
        })
        .and_then(|element| element.get_attribute("href"))
        .and_then(|href| normalize_manifest_href(Some(&href)))
}

#[cfg(target_arch = "wasm32")]
fn service_worker_registration_status(
    registration: &web_sys::ServiceWorkerRegistration,
) -> ServiceWorkerRegistrationStatus {
    ServiceWorkerRegistrationStatus {
        scope: registration.scope(),
        waiting_worker: registration.waiting().is_some(),
        active_worker: registration.active().is_some(),
        installing_worker: registration.installing().is_some(),
    }
}

#[cfg(test)]
mod tests {
    use futures::executor::block_on;

    use super::{
        PwaBrowserContext, PwaEnvironmentStatus, PwaShellMode, classify_shell_mode, icon_192_path,
        icon_512_path, is_install_ready, is_ios_user_agent, is_probably_secure_origin,
        is_update_ready, manifest_path, normalize_manifest_href, offline_fallback_path,
        pwa_guidance_lines, pwa_status_lines, service_worker_path, shell_asset_paths,
    };

    #[test]
    fn secure_origin_detector_accepts_https_and_localhost() {
        assert!(is_probably_secure_origin("https:", "example.org"));
        assert!(is_probably_secure_origin("http:", "localhost"));
        assert!(is_probably_secure_origin("http:", "127.0.0.1"));
        assert!(!is_probably_secure_origin("http:", "example.org"));
    }

    #[test]
    fn manifest_href_is_trimmed_and_empty_values_are_ignored() {
        assert_eq!(
            normalize_manifest_href(Some("  /manifest.webmanifest  ")),
            Some("/manifest.webmanifest".to_string())
        );
        assert_eq!(normalize_manifest_href(Some("   ")), None);
        assert_eq!(normalize_manifest_href(None), None);
    }

    #[test]
    fn preview_lines_include_all_flags() {
        let lines = pwa_status_lines(&PwaEnvironmentStatus {
            secure_context: true,
            online: true,
            service_worker_supported: false,
            service_worker_controlled: false,
            manifest_href: Some("/manifest.webmanifest".to_string()),
            registration_scope: Some("/".to_string()),
            waiting_worker: false,
            active_worker: true,
            active_cache: Some("sp42-shell-v5".to_string()),
            install_prompt_available: true,
            shell_mode: PwaShellMode::BrowserTab,
            browser_context: PwaBrowserContext {
                display_mode_standalone: false,
                ios_device: false,
                browser_label: Some("Chromium".to_string()),
            },
            errors: Vec::new(),
        });

        assert!(
            lines
                .iter()
                .any(|line| line.contains("secure_context=true"))
        );
        assert!(lines.iter().any(|line| line.contains("online=true")));
        assert!(
            lines
                .iter()
                .any(|line| line.contains("service_worker_supported=false"))
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("registration_scope=/"))
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("manifest_href=/manifest.webmanifest"))
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("install_prompt_available=true"))
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("install_ready=false"))
        );
        assert!(lines.iter().any(|line| line.contains("update_ready=false")));
        assert!(
            lines
                .iter()
                .any(|line| line.contains("shell_mode=Browser tab"))
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("browser_context_standalone=false"))
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("active_cache=sp42-shell-v5"))
        );
    }

    #[test]
    fn non_wasm_stubs_return_expected_defaults() {
        assert!(!super::inject_manifest_link());
        assert!(!super::listen_for_install_prompt());
        assert!(!super::listen_for_service_worker_messages());
        assert!(!super::is_install_prompt_available());

        let result = block_on(super::trigger_install_prompt());
        assert!(result.is_err());

        let status = block_on(super::initialize_pwa());
        assert!(!status.secure_context);
        assert!(!status.service_worker_supported);
    }

    #[test]
    fn preview_pwa_environment_returns_safe_defaults_on_native() {
        let status = super::preview_pwa_environment();
        assert!(!status.secure_context);
        assert!(!status.online);
        assert!(!status.service_worker_supported);
        assert!(status.manifest_href.is_none());
        assert!(status.registration_scope.is_none());
        assert!(!status.install_prompt_available);
        assert_eq!(status.shell_mode, PwaShellMode::Unsupported);
    }

    #[test]
    fn asset_paths_are_stable() {
        assert_eq!(manifest_path(), "/manifest.json");
        assert_eq!(service_worker_path(), "/sw.js");
        assert_eq!(offline_fallback_path(), "/offline.html");
        assert_eq!(icon_192_path(), "/icons/sp42-icon-192.svg");
        assert_eq!(icon_512_path(), "/icons/sp42-icon-512.svg");
        assert_eq!(
            shell_asset_paths(),
            [
                "/manifest.json",
                "/sw.js",
                "/offline.html",
                "/icons/sp42-icon-192.svg",
                "/icons/sp42-icon-512.svg",
            ]
        );
    }

    #[test]
    fn readiness_helpers_require_secure_context_and_registration() {
        let ready = PwaEnvironmentStatus {
            secure_context: true,
            online: true,
            service_worker_supported: true,
            service_worker_controlled: true,
            manifest_href: Some("/manifest.json".to_string()),
            registration_scope: Some("/".to_string()),
            waiting_worker: true,
            active_worker: true,
            active_cache: Some("sp42-shell-v5".to_string()),
            install_prompt_available: true,
            shell_mode: PwaShellMode::InstalledStandalone,
            browser_context: PwaBrowserContext {
                display_mode_standalone: true,
                ios_device: false,
                browser_label: Some("Chromium".to_string()),
            },
            errors: Vec::new(),
        };

        assert!(is_install_ready(&ready));
        assert!(is_update_ready(&ready));

        let missing_manifest = PwaEnvironmentStatus {
            manifest_href: None,
            ..ready.clone()
        };
        assert!(!is_install_ready(&missing_manifest));
        assert!(is_update_ready(&missing_manifest));

        let missing_registration = PwaEnvironmentStatus {
            registration_scope: None,
            ..ready
        };
        assert!(!is_install_ready(&missing_registration));
        assert!(!is_update_ready(&missing_registration));
    }

    #[test]
    fn guidance_lines_call_out_ios_and_updates() {
        let status = PwaEnvironmentStatus {
            secure_context: true,
            online: false,
            service_worker_supported: true,
            service_worker_controlled: false,
            manifest_href: Some("/manifest.json".to_string()),
            registration_scope: Some("/".to_string()),
            waiting_worker: true,
            active_worker: true,
            active_cache: None,
            install_prompt_available: false,
            shell_mode: PwaShellMode::IosStandalone,
            browser_context: PwaBrowserContext {
                display_mode_standalone: true,
                ios_device: true,
                browser_label: Some("iOS Safari".to_string()),
            },
            errors: Vec::new(),
        };

        let lines = pwa_guidance_lines(&status);

        assert!(
            lines
                .iter()
                .any(|line| line.contains("iOS standalone mode"))
        );
        assert!(lines.iter().any(|line| line.contains("Add to Home Screen")));
        assert!(
            lines
                .iter()
                .any(|line| line.contains("offline shell and update checks are enabled"))
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("live patrol actions stay suspended"))
        );
    }

    #[test]
    fn ios_user_agents_are_detected() {
        assert!(is_ios_user_agent(
            "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X)"
        ));
        assert!(is_ios_user_agent(
            "Mozilla/5.0 (iPad; CPU OS 16_0 like Mac OS X)"
        ));
        assert!(!is_ios_user_agent("Mozilla/5.0 (X11; Linux x86_64)"));
    }
}
