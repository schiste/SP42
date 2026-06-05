use axum::Router;
use axum::http::HeaderName;
use axum::http::Method;
use axum::http::header::{CONTENT_TYPE, COOKIE};
use axum::middleware;
use axum::routing::get;
use sp42_core::routes as route_contracts;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};

use crate::static_assets::{
    browser_app_dist_dir, browser_shell_unavailable, disable_response_caching, get_favicon,
    get_manifest_json, get_offline_html, get_runtime_config_js, get_service_worker,
    get_static_icon,
};
use crate::{
    AppState, CSRF_HEADER_NAME, coordination_socket, delete_session, get_action_history,
    get_action_status, get_article_inventory, get_auth_callback, get_auth_login, get_auth_session,
    get_bootstrap_status, get_capabilities, get_coordination_inspections,
    get_coordination_room_inspection, get_coordination_room_state, get_coordination_snapshot,
    get_debug_summary, get_healthz, get_live_operator_view, get_logical_storage_document,
    get_operator_readiness, get_operator_report, get_operator_runtime, get_operator_storage_layout,
    get_public_storage_document, get_rendered_hunk_preview, get_revision_diff,
    get_revision_media_diff, get_runtime_debug, get_session, get_storage_document,
    post_auth_logout, post_bootstrap_session, post_execute_action, put_logical_storage_document,
    put_public_storage_document, put_storage_document,
};

pub(crate) fn build_router(state: AppState) -> Router {
    let app_dist_dir = browser_app_dist_dir();
    let browser_shell = if app_dist_dir.join("index.html").is_file() {
        Some(
            ServeDir::new(&app_dist_dir)
                .precompressed_gzip()
                .not_found_service(ServeFile::new(app_dist_dir.join("index.html"))),
        )
    } else {
        None
    };

    let allowed_origins = state.deployment.allowed_origins.clone();
    let router = operator_routes(Router::new())
        .with_state(state)
        .layer(
            CorsLayer::new()
                .allow_credentials(true)
                .allow_origin(AllowOrigin::list(allowed_origins))
                .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
                .allow_headers([
                    CONTENT_TYPE,
                    COOKIE,
                    HeaderName::from_static(CSRF_HEADER_NAME),
                ]),
        )
        .layer(middleware::from_fn(disable_response_caching));

    if let Some(browser_shell) = browser_shell {
        router.fallback_service(browser_shell)
    } else {
        router.route("/", get(browser_shell_unavailable))
    }
}

fn operator_routes(router: Router<AppState>) -> Router<AppState> {
    let router = coordination_routes(router);
    let router = debug_routes(router);
    let router = auth_routes(router);
    let router = operator_api_routes(router);
    let router = operator_storage_routes(router);
    let router = dev_bridge_routes(router);
    static_asset_routes(router)
}

fn coordination_routes(router: Router<AppState>) -> Router<AppState> {
    router
        .route(
            route_contracts::COORDINATION_ROOMS_PATH,
            get(get_coordination_snapshot),
        )
        .route(
            route_contracts::COORDINATION_ROOM_PATTERN,
            get(get_coordination_room_state),
        )
        .route(
            route_contracts::COORDINATION_ROOM_INSPECTION_PATTERN,
            get(get_coordination_room_inspection),
        )
        .route(
            route_contracts::COORDINATION_INSPECTIONS_PATH,
            get(get_coordination_inspections),
        )
        .route(
            route_contracts::COORDINATION_WS_PATTERN,
            get(coordination_socket),
        )
}

fn debug_routes(router: Router<AppState>) -> Router<AppState> {
    router
        .route(route_contracts::DEBUG_SUMMARY_PATH, get(get_debug_summary))
        .route(route_contracts::DEBUG_RUNTIME_PATH, get(get_runtime_debug))
}

fn auth_routes(router: Router<AppState>) -> Router<AppState> {
    router
        .route(route_contracts::AUTH_LOGIN_PATH, get(get_auth_login))
        .route(route_contracts::AUTH_CALLBACK_PATH, get(get_auth_callback))
        .route(route_contracts::AUTH_SESSION_PATH, get(get_auth_session))
        .route(
            route_contracts::AUTH_LOGOUT_PATH,
            axum::routing::post(post_auth_logout),
        )
}

fn operator_api_routes(router: Router<AppState>) -> Router<AppState> {
    router
        .route(
            route_contracts::OPERATOR_READINESS_PATH,
            get(get_operator_readiness),
        )
        .route(
            route_contracts::OPERATOR_REPORT_PATH,
            get(get_operator_report),
        )
        .route(
            route_contracts::OPERATOR_LIVE_PATTERN,
            get(get_live_operator_view),
        )
        .route(
            route_contracts::OPERATOR_ARTICLE_PATTERN,
            get(get_article_inventory),
        )
        .route(
            route_contracts::OPERATOR_DIFF_PATTERN,
            get(get_revision_diff),
        )
        .route(
            route_contracts::OPERATOR_MEDIA_DIFF_PATTERN,
            get(get_revision_media_diff),
        )
        .route(
            route_contracts::OPERATOR_RENDERED_HUNK_PATTERN,
            get(get_rendered_hunk_preview),
        )
        .route(
            route_contracts::OPERATOR_RUNTIME_PATTERN,
            get(get_operator_runtime),
        )
}

fn operator_storage_routes(router: Router<AppState>) -> Router<AppState> {
    router
        .route(
            route_contracts::OPERATOR_STORAGE_LAYOUT_PATTERN,
            get(get_operator_storage_layout),
        )
        .route(
            route_contracts::OPERATOR_STORAGE_DOCUMENT_PATTERN,
            get(get_storage_document).put(put_storage_document),
        )
        .route(
            route_contracts::OPERATOR_STORAGE_LOGICAL_PATTERN,
            get(get_logical_storage_document).put(put_logical_storage_document),
        )
        .route(
            route_contracts::OPERATOR_STORAGE_PUBLIC_PATTERN,
            get(get_public_storage_document).put(put_public_storage_document),
        )
}

fn dev_bridge_routes(router: Router<AppState>) -> Router<AppState> {
    router
        .route(
            route_contracts::DEV_AUTH_SESSION_PATH,
            get(get_session).delete(delete_session),
        )
        .route(
            route_contracts::DEV_AUTH_CAPABILITIES_PATTERN,
            get(get_capabilities),
        )
        .route(
            route_contracts::DEV_ACTION_EXECUTE_PATH,
            axum::routing::post(post_execute_action),
        )
        .route(route_contracts::ACTION_STATUS_PATH, get(get_action_status))
        .route(
            route_contracts::ACTION_HISTORY_PATH,
            get(get_action_history),
        )
        .route(
            route_contracts::DEV_AUTH_BOOTSTRAP_SESSION_PATH,
            axum::routing::post(post_bootstrap_session),
        )
        .route(
            route_contracts::DEV_AUTH_BOOTSTRAP_STATUS_PATH,
            get(get_bootstrap_status),
        )
}

fn static_asset_routes(router: Router<AppState>) -> Router<AppState> {
    router
        .route(route_contracts::HEALTHZ_PATH, get(get_healthz))
        .route(route_contracts::MANIFEST_JSON_PATH, get(get_manifest_json))
        .route(
            route_contracts::RUNTIME_CONFIG_JS_PATH,
            get(get_runtime_config_js),
        )
        .route(
            route_contracts::SERVICE_WORKER_PATH,
            get(get_service_worker),
        )
        .route(route_contracts::OFFLINE_HTML_PATH, get(get_offline_html))
        .route(route_contracts::ICON_PATTERN, get(get_static_icon))
        .route(route_contracts::FAVICON_PATH, get(get_favicon))
}
