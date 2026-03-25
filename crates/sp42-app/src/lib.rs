pub mod inspector;

#[cfg(target_arch = "wasm32")]
pub mod app;
#[cfg(target_arch = "wasm32")]
pub mod components;
#[cfg(target_arch = "wasm32")]
pub mod pages;
#[cfg(target_arch = "wasm32")]
pub mod platform;

#[cfg(target_arch = "wasm32")]
pub fn run_app() {
    use leptos::prelude::*;
    use wasm_bindgen_futures::spawn_local;

    use crate::platform::bootstrap;

    warm_browser_platform_symbols();

    spawn_local(async move {
        let snapshot = bootstrap::collect_browser_bootstrap_snapshot().await;
        for (key, lines) in bootstrap::bootstrap_status_sections(&snapshot) {
            publish_status(key, &lines);
        }
    });

    mount_root_or_body(|| view! { <app::App /> });
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn wasm_start() {
    run_app();
}

#[cfg(target_arch = "wasm32")]
fn warm_browser_platform_symbols() {
    use crate::platform::{auth, bootstrap, coordination, debug, globals, pwa};

    let _ = pwa::manifest_path;
    let _ = pwa::service_worker_path;
    let _ = pwa::offline_fallback_path;
    let _ = pwa::icon_192_path;
    let _ = pwa::icon_512_path;
    let _ = pwa::shell_asset_paths;
    let _ = pwa::is_probably_secure_origin;
    let _ = pwa::preview_pwa_environment;
    let _ = pwa::pwa_status_lines;
    let _ = pwa::inject_manifest_link;
    let _ = pwa::listen_for_install_prompt;
    let _ = pwa::listen_for_service_worker_messages;
    let _ = pwa::trigger_install_prompt;
    let _ = pwa::activate_waiting_service_worker;
    let _ = pwa::is_install_prompt_available;
    let _ = pwa::fetch_pwa_environment_status;
    let _ = pwa::register_service_worker;
    let _ = pwa::fetch_service_worker_registration;
    let _ = pwa::initialize_pwa;

    let _ = globals::browser_window;
    let _ = globals::browser_document;
    let _ = globals::browser_navigator;
    let _ = globals::browser_service_worker_container;
    let _ = globals::browser_location_search;
    let _ = globals::browser_manifest_href;
    let _ = globals::browser_is_secure_context;
    let _ = globals::publish_window_status_lines;

    let _ = auth::preview_browser_auth;
    let _ = auth::callback_preview;
    let _ = auth::bootstrap_status_lines;
    let _ = auth::dev_auth_session_lines;
    let _ = auth::preview_local_oauth_config_status;
    let _ = auth::preview_dev_auth_session_status;
    let _ = auth::preview_dev_auth_bootstrap_status;
    let _ = auth::fetch_dev_auth_session_status;
    let _ = auth::fetch_dev_auth_bootstrap_status;
    let _ = auth::fetch_dev_auth_action_status;
    let _ = auth::fetch_dev_auth_action_history;
    let _ = auth::bootstrap_dev_auth_session;
    let _ = auth::clear_dev_auth_session;

    let _ = coordination::fetch_coordination_snapshot;
    let _ = coordination::fetch_coordination_room_state;
    let _ = coordination::fetch_coordination_inspections;
    let _ = coordination::fetch_coordination_room_inspection;
    let _ = coordination::preview_coordination_snapshot;
    let _ = coordination::coordination_snapshot_lines;
    let _ = coordination::coordination_state_lines;
    let _ = coordination::room_inspection_lines;

    let _ = debug::fetch_server_debug_summary;
    let _ = debug::fetch_runtime_debug_status;
    let _ = debug::fetch_dev_auth_capabilities;
    let _ = debug::preview_dev_auth_capability_report;
    let _ = debug::preview_runtime_debug_status;
    let _ = debug::preview_server_debug_summary;
    let _ = debug::runtime_debug_status_lines;
    let _ = debug::server_debug_summary_lines;

    let _ = bootstrap::collect_browser_bootstrap_snapshot;
    let _ = bootstrap::bootstrap_status_sections;
    let _ = bootstrap::bootstrap_error_lines;
}

#[cfg(target_arch = "wasm32")]
fn publish_status(key: &str, lines: &[String]) {
    let _ = crate::platform::globals::publish_window_status_lines(key, lines);
}

#[cfg(target_arch = "wasm32")]
fn mount_root_or_body<F, N>(f: F)
where
    F: FnOnce() -> N + 'static,
    N: leptos::prelude::IntoView,
{
    let document = crate::platform::globals::browser_document();
    let root = document
        .as_ref()
        .and_then(|doc| doc.get_element_by_id("sp42-app-root"))
        .and_then(|element| wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlElement>(element).ok());

    if let Some(root) = root {
        leptos::mount::mount_to(root, f).forget();
        return;
    }

    let body = document
        .and_then(|doc| doc.body())
        .expect("browser document body should exist for SP42 mount");
    body.set_inner_html("");
    leptos::mount::mount_to(body, f).forget();
}
