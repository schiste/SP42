use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use base64::Engine as _;
use futures::channel::oneshot;
use sp42_core::errors::{EventSourceError, HttpClientError, StorageError, WebSocketError};
use sp42_core::traits::{Clock, EventSource, HttpClient, Rng, Storage, WebSocket};
use sp42_core::types::{HttpMethod, HttpRequest, HttpResponse, ServerSentEvent, WebSocketFrame};
use url::Url;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};

use super::{globals, js_error_message};

#[derive(Debug, Default, Clone)]
pub struct BrowserHttpClient;

#[async_trait]
impl HttpClient for BrowserHttpClient {
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, HttpClientError> {
        let (sender, receiver) = oneshot::channel();

        wasm_bindgen_futures::spawn_local(async move {
            let result =
                async {
                    let mut builder = match request.method {
                        HttpMethod::Get => gloo_net::http::Request::get(request.url.as_ref()),
                        HttpMethod::Post => gloo_net::http::Request::post(request.url.as_ref()),
                        HttpMethod::Put => gloo_net::http::Request::put(request.url.as_ref()),
                        HttpMethod::Patch => gloo_net::http::Request::patch(request.url.as_ref()),
                        HttpMethod::Delete => gloo_net::http::Request::delete(request.url.as_ref()),
                    };

                    for (key, value) in &request.headers {
                        builder = builder.header(key, value);
                    }

                    let response = if request.body.is_empty() {
                        builder.send().await
                    } else {
                        builder
                            .body(request.body.clone())
                            .map_err(|error| HttpClientError::Transport {
                                message: error.to_string(),
                            })?
                            .send()
                            .await
                    }
                    .map_err(|error| HttpClientError::Transport {
                        message: error.to_string(),
                    })?;

                    let status = response.status();
                    let body = response.binary().await.map_err(|error| {
                        HttpClientError::InvalidResponse {
                            message: error.to_string(),
                        }
                    })?;

                    Ok(HttpResponse {
                        status,
                        headers: BTreeMap::new(),
                        body,
                    })
                }
                .await;

            let _ = sender.send(result);
        });

        receiver.await.map_err(|_| HttpClientError::Transport {
            message: "browser request task was cancelled".to_string(),
        })?
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct BrowserClock;

impl Clock for BrowserClock {
    fn now_ms(&self) -> i64 {
        format!("{:.0}", js_sys::Date::now())
            .parse::<i64>()
            .unwrap_or(i64::MAX)
    }
}

#[derive(Debug, Clone, Default)]
pub struct BrowserRng {
    fallback_counter: u64,
}

impl Rng for BrowserRng {
    fn next_u64(&mut self) -> u64 {
        let mut bytes = [0_u8; 8];

        let crypto_result = globals::browser_window()
            .and_then(|window| window.crypto().ok())
            .and_then(|crypto| crypto.get_random_values_with_u8_array(&mut bytes).ok());

        if crypto_result.is_some() {
            return u64::from_le_bytes(bytes);
        }

        self.fallback_counter = self.fallback_counter.saturating_add(1);
        let now = Clock::now_ms(&BrowserClock);
        let now_bits = u64::from_le_bytes(now.to_le_bytes());
        now_bits ^ self.fallback_counter
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LocalStorageBrowserStorage;

#[async_trait]
impl Storage for LocalStorageBrowserStorage {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let Some(storage) = browser_local_storage()? else {
            return Ok(None);
        };

        let Some(value) = storage
            .get_item(key)
            .map_err(|error| StorageError::Operation {
                message: js_error_message(error),
            })?
        else {
            return Ok(None);
        };

        base64::engine::general_purpose::STANDARD
            .decode(value)
            .map(Some)
            .map_err(|error| StorageError::Operation {
                message: format!("localStorage decode failed: {error}"),
            })
    }

    async fn set(&self, key: String, value: Vec<u8>) -> Result<(), StorageError> {
        let Some(storage) = browser_local_storage()? else {
            return Err(StorageError::Operation {
                message: "window.localStorage is unavailable".to_string(),
            });
        };

        let encoded = base64::engine::general_purpose::STANDARD.encode(value);
        storage
            .set_item(&key, &encoded)
            .map_err(|error| StorageError::Operation {
                message: js_error_message(error),
            })
    }

    async fn remove(&self, key: &str) -> Result<(), StorageError> {
        let Some(storage) = browser_local_storage()? else {
            return Ok(());
        };

        storage
            .remove_item(key)
            .map_err(|error| StorageError::Operation {
                message: js_error_message(error),
            })
    }
}

#[derive(Debug, Clone, Default)]
pub struct VolatileBrowserStorage {
    values: Arc<Mutex<BTreeMap<String, Vec<u8>>>>,
}

#[async_trait]
impl Storage for VolatileBrowserStorage {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let values = self
            .values
            .lock()
            .map_err(|_| StorageError::StatePoisoned {
                resource: "volatile_browser_storage.values",
            })?;

        Ok(values.get(key).cloned())
    }

    async fn set(&self, key: String, value: Vec<u8>) -> Result<(), StorageError> {
        let mut values = self
            .values
            .lock()
            .map_err(|_| StorageError::StatePoisoned {
                resource: "volatile_browser_storage.values",
            })?;

        values.insert(key, value);
        Ok(())
    }

    async fn remove(&self, key: &str) -> Result<(), StorageError> {
        let mut values = self
            .values
            .lock()
            .map_err(|_| StorageError::StatePoisoned {
                resource: "volatile_browser_storage.values",
            })?;

        values.remove(key);
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct BrowserEventSource {
    id: u64,
    queue: Arc<Mutex<VecDeque<Result<ServerSentEvent, EventSourceError>>>>,
}

#[derive(Debug, Clone)]
pub struct BrowserWebSocket {
    id: u64,
    queue: Arc<Mutex<VecDeque<Result<WebSocketFrame, WebSocketError>>>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeEnvironmentStatus {
    pub http_client: &'static str,
    pub event_source: &'static str,
    pub websocket: &'static str,
    pub clock_now_ms: i64,
    pub rng_state_preview: String,
    pub persistent_storage: &'static str,
    pub volatile_storage: &'static str,
}

thread_local! {
    static NEXT_RUNTIME_HANDLE_ID: Cell<u64> = const { Cell::new(1) };
    static EVENT_SOURCE_HANDLES: RefCell<BTreeMap<u64, BrowserEventSourceHandle>> = const {
        RefCell::new(BTreeMap::new())
    };
    static WEB_SOCKET_HANDLES: RefCell<BTreeMap<u64, BrowserWebSocketHandle>> = const {
        RefCell::new(BTreeMap::new())
    };
}

struct BrowserEventSourceHandle {
    base_url: String,
    source: web_sys::EventSource,
    _on_message: Closure<dyn FnMut(web_sys::MessageEvent)>,
    _on_error: Closure<dyn FnMut(web_sys::Event)>,
}

struct BrowserWebSocketHandle {
    socket: web_sys::WebSocket,
    _on_message: Closure<dyn FnMut(web_sys::MessageEvent)>,
    _on_error: Closure<dyn FnMut(web_sys::Event)>,
    _on_close: Closure<dyn FnMut(web_sys::CloseEvent)>,
}

impl BrowserEventSource {
    pub fn connect(url: &Url) -> Result<Self, EventSourceError> {
        let id = next_runtime_handle_id();
        let queue = Arc::new(Mutex::new(VecDeque::new()));
        let handle = build_event_source_handle(url.as_ref(), Arc::clone(&queue))?;

        EVENT_SOURCE_HANDLES.with(|handles| {
            handles.borrow_mut().insert(id, handle);
        });

        Ok(Self { id, queue })
    }
}

impl Drop for BrowserEventSource {
    fn drop(&mut self) {
        EVENT_SOURCE_HANDLES.with(|handles| {
            if let Some(handle) = handles.borrow_mut().remove(&self.id) {
                handle.source.close();
            }
        });
    }
}

#[async_trait]
impl EventSource for BrowserEventSource {
    async fn next_event(&mut self) -> Result<Option<ServerSentEvent>, EventSourceError> {
        let mut queue = self
            .queue
            .lock()
            .map_err(|_| EventSourceError::StatePoisoned {
                resource: "browser_event_source.queue",
            })?;

        match queue.pop_front() {
            Some(result) => result.map(Some),
            None => Ok(None),
        }
    }

    async fn reconnect(&mut self, last_event_id: Option<String>) -> Result<(), EventSourceError> {
        let base_url = EVENT_SOURCE_HANDLES.with(|handles| {
            handles
                .borrow()
                .get(&self.id)
                .map(|handle| handle.base_url.clone())
        });

        let Some(base_url) = base_url else {
            return Err(EventSourceError::Disconnected {
                message: "event source handle is unavailable".to_string(),
            });
        };

        let reopen_url = apply_last_event_id_hint(&base_url, last_event_id.as_deref())?;
        let handle = build_event_source_handle(&reopen_url, Arc::clone(&self.queue))?;

        EVENT_SOURCE_HANDLES.with(|handles| {
            if let Some(old_handle) = handles.borrow_mut().insert(self.id, handle) {
                old_handle.source.close();
            }
        });

        Ok(())
    }
}

impl BrowserWebSocket {
    pub fn connect(url: &Url) -> Result<Self, WebSocketError> {
        let id = next_runtime_handle_id();
        let queue = Arc::new(Mutex::new(VecDeque::new()));
        let handle = build_websocket_handle(url.as_ref(), Arc::clone(&queue))?;

        WEB_SOCKET_HANDLES.with(|handles| {
            handles.borrow_mut().insert(id, handle);
        });

        Ok(Self { id, queue })
    }
}

impl Drop for BrowserWebSocket {
    fn drop(&mut self) {
        WEB_SOCKET_HANDLES.with(|handles| {
            if let Some(handle) = handles.borrow_mut().remove(&self.id) {
                let _ = handle.socket.close();
            }
        });
    }
}

#[async_trait]
impl WebSocket for BrowserWebSocket {
    async fn send(&mut self, frame: WebSocketFrame) -> Result<(), WebSocketError> {
        WEB_SOCKET_HANDLES.with(|handles| {
            let handles = handles.borrow();
            let Some(handle) = handles.get(&self.id) else {
                return Err(WebSocketError::Transport {
                    message: "websocket handle is unavailable".to_string(),
                });
            };

            match frame {
                WebSocketFrame::Text(text) => {
                    handle
                        .socket
                        .send_with_str(&text)
                        .map_err(|error| WebSocketError::Transport {
                            message: js_error_message(error),
                        })
                }
                WebSocketFrame::Binary(bytes) => {
                    handle.socket.send_with_u8_array(&bytes).map_err(|error| {
                        WebSocketError::Transport {
                            message: js_error_message(error),
                        }
                    })
                }
                WebSocketFrame::Close => {
                    handle
                        .socket
                        .close()
                        .map_err(|error| WebSocketError::Transport {
                            message: js_error_message(error),
                        })
                }
            }
        })
    }

    async fn receive(&mut self) -> Result<Option<WebSocketFrame>, WebSocketError> {
        let mut queue = self
            .queue
            .lock()
            .map_err(|_| WebSocketError::StatePoisoned {
                resource: "browser_websocket.queue",
            })?;

        match queue.pop_front() {
            Some(result) => result.map(Some),
            None => Ok(None),
        }
    }
}

#[must_use]
pub fn preview_runtime_environment() -> Vec<String> {
    let status = runtime_environment_status();

    vec![
        format!("http_client={}", status.http_client),
        format!("event_source={}", status.event_source),
        format!("websocket={}", status.websocket),
        format!("clock_now_ms={}", status.clock_now_ms),
        format!("rng_state_preview={}...", status.rng_state_preview),
        format!("persistent_storage={}", status.persistent_storage),
        format!("volatile_storage={}", status.volatile_storage),
    ]
}

#[must_use]
pub fn runtime_environment_status() -> RuntimeEnvironmentStatus {
    let _http_client = BrowserHttpClient;
    let clock = BrowserClock;
    let mut rng = BrowserRng::default();
    let _local_storage = LocalStorageBrowserStorage;
    let volatile_storage = VolatileBrowserStorage::default();
    let entropy_preview = sp42_core::generate_oauth_state(&mut rng);
    let local_storage_available = browser_local_storage().ok().flatten().is_some();
    let _event_source_constructor: fn(&Url) -> Result<BrowserEventSource, EventSourceError> =
        BrowserEventSource::connect;
    let _websocket_constructor: fn(&Url) -> Result<BrowserWebSocket, WebSocketError> =
        BrowserWebSocket::connect;

    RuntimeEnvironmentStatus {
        http_client: "browser-fetch",
        event_source: "native-browser-sse",
        websocket: "native-browser-websocket",
        clock_now_ms: clock.now_ms(),
        rng_state_preview: entropy_preview.chars().take(10).collect::<String>(),
        persistent_storage: if local_storage_available {
            "window.localStorage"
        } else {
            "unavailable"
        },
        volatile_storage: if Arc::strong_count(&volatile_storage.values) >= 1 {
            "memory"
        } else {
            "unknown"
        },
    }
}

fn browser_local_storage() -> Result<Option<web_sys::Storage>, StorageError> {
    globals::browser_window()
        .ok_or_else(|| StorageError::Operation {
            message: "window is unavailable".to_string(),
        })?
        .local_storage()
        .map_err(|error| StorageError::Operation {
            message: js_error_message(error),
        })
}

fn next_runtime_handle_id() -> u64 {
    NEXT_RUNTIME_HANDLE_ID.with(|next_id| {
        let current = next_id.get();
        next_id.set(current.saturating_add(1));
        current
    })
}

fn build_event_source_handle(
    url: &str,
    queue: Arc<Mutex<VecDeque<Result<ServerSentEvent, EventSourceError>>>>,
) -> Result<BrowserEventSourceHandle, EventSourceError> {
    let source =
        web_sys::EventSource::new(url).map_err(|error| EventSourceError::Disconnected {
            message: js_error_message(error),
        })?;

    let on_message_queue = Arc::clone(&queue);
    let on_message =
        Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |event: web_sys::MessageEvent| {
            let event_id = event.last_event_id();
            let payload = stringify_js_value(&event.data());
            if let Ok(mut guard) = on_message_queue.lock() {
                guard.push_back(Ok(ServerSentEvent {
                    event_type: Some("message".to_string()),
                    id: (!event_id.is_empty()).then_some(event_id),
                    data: payload,
                    retry_ms: None,
                }));
            }
        });
    source.set_onmessage(Some(on_message.as_ref().unchecked_ref()));

    let on_error_queue = Arc::clone(&queue);
    let on_error = Closure::<dyn FnMut(web_sys::Event)>::new(move |_event: web_sys::Event| {
        if let Ok(mut guard) = on_error_queue.lock() {
            guard.push_back(Err(EventSourceError::Disconnected {
                message: "browser event source reported an error".to_string(),
            }));
        }
    });
    source.set_onerror(Some(on_error.as_ref().unchecked_ref()));

    Ok(BrowserEventSourceHandle {
        base_url: url.to_string(),
        source,
        _on_message: on_message,
        _on_error: on_error,
    })
}

fn build_websocket_handle(
    url: &str,
    queue: Arc<Mutex<VecDeque<Result<WebSocketFrame, WebSocketError>>>>,
) -> Result<BrowserWebSocketHandle, WebSocketError> {
    let socket = web_sys::WebSocket::new(url).map_err(|error| WebSocketError::Transport {
        message: js_error_message(error),
    })?;
    socket.set_binary_type(web_sys::BinaryType::Arraybuffer);

    let on_message_queue = Arc::clone(&queue);
    let on_message =
        Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |event: web_sys::MessageEvent| {
            let payload = parse_websocket_frame(event.data());
            if let Ok(mut guard) = on_message_queue.lock() {
                guard.push_back(payload);
            }
        });
    socket.set_onmessage(Some(on_message.as_ref().unchecked_ref()));

    let on_error_queue = Arc::clone(&queue);
    let on_error = Closure::<dyn FnMut(web_sys::Event)>::new(move |_event: web_sys::Event| {
        if let Ok(mut guard) = on_error_queue.lock() {
            guard.push_back(Err(WebSocketError::Transport {
                message: "browser websocket reported an error".to_string(),
            }));
        }
    });
    socket.set_onerror(Some(on_error.as_ref().unchecked_ref()));

    let on_close_queue = Arc::clone(&queue);
    let on_close = Closure::<dyn FnMut(web_sys::CloseEvent)>::new(move |_event| {
        if let Ok(mut guard) = on_close_queue.lock() {
            guard.push_back(Ok(WebSocketFrame::Close));
        }
    });
    socket.set_onclose(Some(on_close.as_ref().unchecked_ref()));

    Ok(BrowserWebSocketHandle {
        socket,
        _on_message: on_message,
        _on_error: on_error,
        _on_close: on_close,
    })
}

fn apply_last_event_id_hint(
    url: &str,
    last_event_id: Option<&str>,
) -> Result<String, EventSourceError> {
    let Some(last_event_id) = last_event_id else {
        return Ok(url.to_string());
    };

    let mut parsed = Url::parse(url).map_err(|error| EventSourceError::Disconnected {
        message: format!("invalid event source url: {error}"),
    })?;
    parsed
        .query_pairs_mut()
        .append_pair("sp42_last_event_id", last_event_id);
    Ok(parsed.to_string())
}

fn parse_websocket_frame(payload: JsValue) -> Result<WebSocketFrame, WebSocketError> {
    if let Some(text) = payload.as_string() {
        return Ok(WebSocketFrame::Text(text));
    }

    if let Ok(buffer) = payload.dyn_into::<js_sys::ArrayBuffer>() {
        let array = js_sys::Uint8Array::new(&buffer);
        let mut bytes = vec![0_u8; usize::try_from(array.length()).unwrap_or(0)];
        array.copy_to(&mut bytes);
        return Ok(WebSocketFrame::Binary(bytes));
    }

    Err(WebSocketError::Transport {
        message: "unsupported websocket payload type".to_string(),
    })
}

fn stringify_js_value(value: &JsValue) -> String {
    value
        .as_string()
        .or_else(|| {
            js_sys::JSON::stringify(value)
                .ok()
                .and_then(|text| text.as_string())
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use futures::executor::block_on;
    use sp42_core::traits::{Clock, Rng, Storage};

    use super::{
        BrowserClock, BrowserRng, LocalStorageBrowserStorage, VolatileBrowserStorage,
        preview_runtime_environment, runtime_environment_status,
    };

    #[test]
    fn clock_returns_non_negative_timestamp() {
        let clock = BrowserClock;
        assert!(clock.now_ms() >= 0);
    }

    #[test]
    fn rng_returns_progressive_values() {
        let mut rng = BrowserRng::default();
        let first = rng.next_u64();
        let second = rng.next_u64();
        assert_ne!(first ^ second, 0);
    }

    #[test]
    fn volatile_storage_round_trips() {
        let storage = VolatileBrowserStorage::default();
        block_on(storage.set("key".to_string(), b"value".to_vec())).expect("set should succeed");
        let value = block_on(storage.get("key")).expect("get should succeed");

        assert_eq!(value, Some(b"value".to_vec()));
    }

    #[test]
    fn local_storage_handles_missing_window_gracefully() {
        let storage = LocalStorageBrowserStorage;
        let result = block_on(storage.get("missing"));
        assert!(result.is_ok());
    }

    #[test]
    fn runtime_preview_contains_entries() {
        let lines = preview_runtime_environment();
        assert!(lines.len() >= 6);
        assert!(lines.iter().any(|line| line.contains("http_client=")));
        assert!(lines.iter().any(|line| line.contains("event_source=")));
        assert!(lines.iter().any(|line| line.contains("websocket=")));
        assert!(lines.iter().any(|line| line.contains("clock_now_ms=")));
    }

    #[test]
    fn runtime_status_contains_field_level_data() {
        let status = runtime_environment_status();

        assert_eq!(status.http_client, "browser-fetch");
        assert_eq!(status.event_source, "native-browser-sse");
        assert_eq!(status.websocket, "native-browser-websocket");
        assert!(!status.rng_state_preview.is_empty());
    }
}
