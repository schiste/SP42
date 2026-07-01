use std::path::PathBuf;

use axum::{
    Json,
    extract::{Path, State},
    http::{
        HeaderValue, StatusCode,
        header::{CACHE_CONTROL, CONTENT_TYPE, PRAGMA},
    },
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
};
use sp42_core::routes as route_contracts;

use crate::AppState;

pub(crate) async fn disable_response_caching(
    request: axum::extract::Request,
    next: Next,
) -> Response {
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        CACHE_CONTROL,
        HeaderValue::from_static("no-store, no-cache, must-revalidate, max-age=0"),
    );
    response
        .headers_mut()
        .insert(PRAGMA, HeaderValue::from_static("no-cache"));
    response
}

pub(crate) fn browser_app_dist_dir() -> PathBuf {
    std::env::var_os("SP42_APP_DIST_DIR").map_or_else(
        || {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("..")
                .join("target")
                .join("dist")
                .join("sp42-app")
        },
        PathBuf::from,
    )
}

fn app_static_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("sp42-app")
        .join("static")
}

fn ui_static_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("sp42-ui")
        .join("static")
}

pub(crate) async fn browser_shell_unavailable() -> impl IntoResponse {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        [(
            CONTENT_TYPE,
            HeaderValue::from_static("text/html; charset=utf-8"),
        )],
        r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>SP42 frontend unavailable</title>
  </head>
  <body>
    <main>
      <h1>SP42 frontend build missing</h1>
      <p>Build the browser app first:</p>
      <p><code>./scripts/build-frontend.sh</code></p>
      <p>Or run live development from the repository root with <code>trunk serve</code>.</p>
    </main>
  </body>
</html>"#,
    )
}

fn static_asset_path(file_name: &str) -> PathBuf {
    let dist_candidate = browser_app_dist_dir().join(file_name);
    if dist_candidate.is_file() {
        dist_candidate
    } else if file_name == "sw.js" {
        app_static_dir().join(file_name)
    } else {
        ui_static_dir().join(file_name)
    }
}

pub(crate) async fn get_manifest_json() -> impl IntoResponse {
    serve_static_file(
        static_asset_path("manifest.json"),
        "application/manifest+json",
    )
    .await
}

pub(crate) async fn get_runtime_config_js(State(state): State<AppState>) -> impl IntoResponse {
    let payload = serde_json::json!({
        "defaultWikiId": state.default_wiki_id(),
        "deploymentMode": state.deployment.mode.as_str(),
    });
    let serialized = serde_json::to_string(&payload).expect("runtime config should serialize");
    (
        [(
            CONTENT_TYPE,
            HeaderValue::from_static("application/javascript"),
        )],
        format!(
            "window.__SP42_RUNTIME_CONFIG__ = {{ ...(window.__SP42_RUNTIME_CONFIG__ || {{}}), ...{serialized} }};\n"
        ),
    )
}

/// The full list of resolvable Wikimedia `wiki_id`s (the embedded `SiteMatrix`
/// snapshot, ADR-0014), for the workspace wiki picker's filterable dropdown.
pub(crate) async fn get_wikis() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "wiki_ids": sp42_wiki::known_wiki_ids() }))
}

/// The resolved default namespace allowlist for one wiki, so the patrol filter UI
/// shows the same namespaces the server uses for an unfiltered query — configured
/// wikis (e.g. enwiki `[0,1]`) differ from the universal default. Falls back to
/// the shared patrol default for an unknown id. Codex review #90.
pub(crate) async fn get_wiki_defaults(
    State(state): State<AppState>,
    Path(wiki_id): Path<String>,
) -> Json<serde_json::Value> {
    let namespace_allowlist = state.wiki_registry.resolve(&wiki_id).map_or_else(
        |_| sp42_core::DEFAULT_PATROL_NAMESPACES.to_vec(),
        |config| config.namespace_allowlist,
    );
    Json(serde_json::json!({ "namespace_allowlist": namespace_allowlist }))
}

pub(crate) async fn get_service_worker() -> impl IntoResponse {
    serve_static_file(static_asset_path("sw.js"), "application/javascript").await
}

pub(crate) async fn get_offline_html() -> impl IntoResponse {
    serve_static_file(
        static_asset_path("offline.html"),
        "text/html; charset=utf-8",
    )
    .await
}

pub(crate) async fn get_static_icon(Path(icon_name): Path<String>) -> impl IntoResponse {
    let candidate = ui_static_dir().join("icons").join(&icon_name);
    if candidate.is_file() {
        serve_static_file(candidate, "image/svg+xml").await
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

pub(crate) async fn get_favicon() -> impl IntoResponse {
    Redirect::temporary(route_contracts::SP42_ICON_192_PATH)
}

async fn serve_static_file(path: PathBuf, content_type: &'static str) -> Response {
    match tokio::fs::read(path).await {
        Ok(bytes) => (
            StatusCode::OK,
            [(CONTENT_TYPE, HeaderValue::from_static(content_type))],
            bytes,
        )
            .into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}
