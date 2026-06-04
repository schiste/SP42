use std::path::PathBuf;

use axum::{
    extract::{Path, State},
    http::{
        HeaderValue, StatusCode,
        header::{CACHE_CONTROL, CONTENT_TYPE, PRAGMA},
    },
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
};

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
    <style>
      body {
        margin: 0;
        min-height: 100vh;
        display: grid;
        place-items: center;
        background: #091321;
        color: #e3edf9;
        font-family: "IBM Plex Sans", "Avenir Next", sans-serif;
      }
      main {
        width: min(42rem, calc(100vw - 3rem));
        padding: 1.5rem;
        border: 1px solid rgba(227, 237, 249, 0.16);
        border-radius: 1rem;
        background: rgba(15, 28, 46, 0.92);
      }
      code {
        color: #52c7b8;
      }
    </style>
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
    } else {
        app_static_dir().join(file_name)
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
    let candidate = app_static_dir().join("icons").join(&icon_name);
    if candidate.is_file() {
        serve_static_file(candidate, "image/svg+xml").await
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

pub(crate) async fn get_favicon() -> impl IntoResponse {
    Redirect::temporary("/icons/sp42-icon-192.svg")
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
