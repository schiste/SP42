use axum::Router;
use axum::http::HeaderName;
use axum::http::Method;
use axum::http::header::{CONTENT_TYPE, COOKIE};
use axum::middleware;
use axum::routing::get;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};

use crate::{
    ACTION_HISTORY_PATH, ACTION_STATUS_PATH, AUTH_CALLBACK_PATH, AUTH_LOGIN_PATH, AUTH_LOGOUT_PATH,
    AUTH_SESSION_PATH, AppState, CSRF_HEADER_NAME, OPERATOR_READINESS_PATH, OPERATOR_REPORT_PATH,
    browser_app_dist_dir, browser_shell_unavailable, coordination_socket, delete_session,
    disable_response_caching, get_action_history, get_action_status, get_article_inventory,
    get_auth_callback, get_auth_login, get_auth_session, get_bootstrap_status, get_capabilities,
    get_coordination_inspections, get_coordination_room_inspection, get_coordination_room_state,
    get_coordination_snapshot, get_debug_summary, get_favicon, get_healthz, get_live_operator_view,
    get_logical_storage_document, get_manifest_json, get_offline_html, get_operator_readiness,
    get_operator_report, get_operator_runtime, get_operator_storage_layout,
    get_public_storage_document, get_rendered_hunk_preview, get_revision_diff,
    get_revision_media_diff, get_runtime_config_js, get_runtime_debug, get_service_worker,
    get_session, get_static_icon, get_storage_document, post_auth_logout, post_bootstrap_session,
    post_execute_action, put_logical_storage_document, put_public_storage_document,
    put_storage_document,
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
    router
        .route("/coordination/rooms", get(get_coordination_snapshot))
        .route(
            "/coordination/rooms/{wiki_id}",
            get(get_coordination_room_state),
        )
        .route(
            "/coordination/rooms/{wiki_id}/inspection",
            get(get_coordination_room_inspection),
        )
        .route(
            "/coordination/inspections",
            get(get_coordination_inspections),
        )
        .route("/debug/summary", get(get_debug_summary))
        .route("/debug/runtime", get(get_runtime_debug))
        .route(AUTH_LOGIN_PATH, get(get_auth_login))
        .route(AUTH_CALLBACK_PATH, get(get_auth_callback))
        .route(AUTH_SESSION_PATH, get(get_auth_session))
        .route(AUTH_LOGOUT_PATH, axum::routing::post(post_auth_logout))
        .route(OPERATOR_READINESS_PATH, get(get_operator_readiness))
        .route(OPERATOR_REPORT_PATH, get(get_operator_report))
        .route("/operator/live/{wiki_id}", get(get_live_operator_view))
        .route("/operator/article/{wiki_id}", get(get_article_inventory))
        .route(
            "/operator/diff/{wiki_id}/{rev_id}/{old_rev_id}",
            get(get_revision_diff),
        )
        .route(
            "/operator/media-diff/{wiki_id}/{rev_id}/{old_rev_id}",
            get(get_revision_media_diff),
        )
        .route(
            "/operator/rendered-hunk/{wiki_id}/{rev_id}/{old_rev_id}/{hunk_index}",
            get(get_rendered_hunk_preview),
        )
        .route("/operator/runtime/{wiki_id}", get(get_operator_runtime))
        .route(
            "/operator/storage/layout/{wiki_id}",
            get(get_operator_storage_layout),
        )
        .route(
            "/operator/storage/document/{wiki_id}",
            get(get_storage_document).put(put_storage_document),
        )
        .route(
            "/operator/storage/logical/{wiki_id}/{realm}/{kind}",
            get(get_logical_storage_document).put(put_logical_storage_document),
        )
        .route(
            "/operator/storage/public/{wiki_id}/{kind}",
            get(get_public_storage_document).put(put_public_storage_document),
        )
        .route("/ws/{wiki_id}", get(coordination_socket))
        .route("/dev/auth/session", get(get_session).delete(delete_session))
        .route("/dev/auth/capabilities/{wiki_id}", get(get_capabilities))
        .route(
            "/dev/actions/execute",
            axum::routing::post(post_execute_action),
        )
        .route(ACTION_STATUS_PATH, get(get_action_status))
        .route(ACTION_HISTORY_PATH, get(get_action_history))
        .route(
            "/dev/auth/session/bootstrap",
            axum::routing::post(post_bootstrap_session),
        )
        .route("/dev/auth/bootstrap/status", get(get_bootstrap_status))
        .route("/healthz", get(get_healthz))
        .route("/manifest.json", get(get_manifest_json))
        .route("/runtime-config.js", get(get_runtime_config_js))
        .route("/sw.js", get(get_service_worker))
        .route("/offline.html", get(get_offline_html))
        .route("/icons/{icon_name}", get(get_static_icon))
        .route("/favicon.ico", get(get_favicon))
}
