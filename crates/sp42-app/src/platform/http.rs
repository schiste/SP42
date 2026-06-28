#[cfg(target_arch = "wasm32")]
use std::cell::RefCell;

#[cfg(target_arch = "wasm32")]
use gloo_net::http::{Request, RequestBuilder};
#[cfg(target_arch = "wasm32")]
use web_sys::RequestCredentials;

#[cfg(target_arch = "wasm32")]
use sp42_core::routes::CSRF_HEADER_NAME;

#[cfg(target_arch = "wasm32")]
thread_local! {
    static CSRF_TOKEN: RefCell<Option<String>> = const { RefCell::new(None) };
    // Single re-gate trigger: invoked whenever any API call receives HTTP 401,
    // so the auth gate drops to login regardless of which call detected the
    // expired session — the server session is the one authority. Codex review #90.
    static ON_UNAUTHORIZED: RefCell<Option<Box<dyn Fn()>>> = const { RefCell::new(None) };
}

/// Register the handler invoked on any API `401`. The app installs one at mount
/// (flip the auth state to anonymous); mirrors the CSRF thread-local pattern.
#[cfg(target_arch = "wasm32")]
pub(crate) fn set_unauthorized_handler(handler: impl Fn() + 'static) {
    ON_UNAUTHORIZED.with(|cell| {
        *cell.borrow_mut() = Some(Box::new(handler));
    });
}

#[cfg(target_arch = "wasm32")]
fn notify_unauthorized() {
    ON_UNAUTHORIZED.with(|cell| {
        if let Some(handler) = cell.borrow().as_ref() {
            handler();
        }
    });
}

/// Turn a completed response into bytes-or-error. A `401` means the server
/// session is gone, so it re-gates to login through the single registered
/// handler rather than each call site sniffing the error string. Codex review #90.
#[cfg(target_arch = "wasm32")]
fn finish_response(status: u16, bytes: Vec<u8>, context: &str) -> Result<Vec<u8>, String> {
    if is_success_status(status) {
        return Ok(bytes);
    }
    if status == 401 {
        notify_unauthorized();
    }
    Err(format_http_error(context, status, &bytes))
}

/// Returns `true` when the status code is a successful HTTP response.
#[must_use]
pub(crate) fn is_success_status(status: u16) -> bool {
    (200..300).contains(&status)
}

/// Formats a browser HTTP failure with context and response body.
#[must_use]
pub(crate) fn format_http_error(context: &str, status: u16, body: &[u8]) -> String {
    let body = String::from_utf8_lossy(body);
    if body.trim().is_empty() {
        format!("{context}: HTTP {status}")
    } else {
        format!("{context}: HTTP {status}: {body}")
    }
}

#[cfg(target_arch = "wasm32")]
async fn request_bytes(builder: RequestBuilder, context: &str) -> Result<Vec<u8>, String> {
    let request = include_credentials(builder)
        .build()
        .map_err(|error| format!("{context}: {error}"))?;
    let response = request
        .send()
        .await
        .map_err(|error| format!("{context}: {}", error))?;
    let status = response.status();
    let bytes = response
        .binary()
        .await
        .map_err(|error| format!("{context}: {}", error))?;

    finish_response(status, bytes, context)
}

#[cfg(target_arch = "wasm32")]
async fn request_bytes_with_csrf(
    builder: RequestBuilder,
    context: &str,
) -> Result<Vec<u8>, String> {
    request_bytes(include_csrf_token(builder), context).await
}

#[cfg(target_arch = "wasm32")]
fn include_credentials(request: RequestBuilder) -> RequestBuilder {
    request.credentials(RequestCredentials::Include)
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn remember_csrf_token(token: Option<&str>) {
    CSRF_TOKEN.with(|cell| {
        *cell.borrow_mut() = token
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
    });
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn forget_csrf_token() {
    remember_csrf_token(None);
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn include_csrf_token(request: RequestBuilder) -> RequestBuilder {
    match csrf_token_header_value() {
        Some(token) => request.header(CSRF_HEADER_NAME, &token),
        None => request,
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn csrf_token_header_value() -> Option<String> {
    CSRF_TOKEN.with(|cell| cell.borrow().clone())
}

#[cfg(target_arch = "wasm32")]
pub(crate) async fn get_bytes(url: &str, context: &str) -> Result<Vec<u8>, String> {
    request_bytes(Request::get(url), context).await
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) async fn get_bytes(_url: &str, context: &str) -> Result<Vec<u8>, String> {
    Err(format!("{context}: browser-only helper"))
}

#[cfg(target_arch = "wasm32")]
pub(crate) async fn delete_bytes(url: &str, context: &str) -> Result<Vec<u8>, String> {
    request_bytes_with_csrf(Request::delete(url), context).await
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) async fn delete_bytes(_url: &str, context: &str) -> Result<Vec<u8>, String> {
    Err(format!("{context}: browser-only helper"))
}

#[cfg(target_arch = "wasm32")]
pub(crate) async fn post_json_bytes(
    url: &str,
    body: String,
    context: &str,
) -> Result<Vec<u8>, String> {
    let request = include_csrf_token(include_credentials(Request::post(url)))
        .header("content-type", "application/json")
        .body(body)
        .map_err(|error| format!("{context}: {}", error))?;
    let response = request
        .send()
        .await
        .map_err(|error| format!("{context}: {}", error))?;
    let status = response.status();
    let bytes = response
        .binary()
        .await
        .map_err(|error| format!("{context}: {}", error))?;

    finish_response(status, bytes, context)
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) async fn post_json_bytes(
    _url: &str,
    _body: String,
    context: &str,
) -> Result<Vec<u8>, String> {
    Err(format!("{context}: browser-only helper"))
}

#[cfg(test)]
mod tests {
    use super::{format_http_error, is_success_status};

    #[test]
    fn success_status_only_matches_2xx() {
        assert!(is_success_status(200));
        assert!(is_success_status(204));
        assert!(is_success_status(299));
        assert!(!is_success_status(199));
        assert!(!is_success_status(300));
    }

    #[test]
    fn formats_http_error_with_context_and_body() {
        let message = format_http_error("fetch demo", 418, b"teapot");

        assert_eq!(message, "fetch demo: HTTP 418: teapot");
    }

    // Only a 401 invokes the single re-gate handler; other errors and successes
    // leave auth state untouched. Codex review #90.
    #[cfg(target_arch = "wasm32")]
    #[test]
    fn only_401_triggers_the_unauthorized_handler() {
        use std::cell::Cell;
        use std::rc::Rc;

        let fired = Rc::new(Cell::new(0u32));
        let counter = fired.clone();
        super::set_unauthorized_handler(move || counter.set(counter.get() + 1));

        assert!(super::finish_response(401, b"gone".to_vec(), "ctx").is_err());
        assert_eq!(fired.get(), 1);

        assert!(super::finish_response(500, b"boom".to_vec(), "ctx").is_err());
        assert_eq!(fired.get(), 1, "non-401 errors must not re-gate");

        assert_eq!(
            super::finish_response(200, b"ok".to_vec(), "ctx").expect("success"),
            b"ok"
        );
        assert_eq!(fired.get(), 1, "success must not re-gate");
    }
}
