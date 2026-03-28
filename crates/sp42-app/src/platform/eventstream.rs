/// Browser-side EventStreams SSE client.
///
/// Connects directly to stream.wikimedia.org from the browser and
/// delivers filtered recent-change events for a target wiki.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

/// Parsed recent-change event from Wikimedia EventStreams.
#[derive(Debug, Clone)]
pub struct StreamEvent {
    pub wiki: String,
    pub title: String,
    pub namespace: i32,
    pub rev_id: u64,
    pub old_rev_id: Option<u64>,
    pub user: String,
    pub bot: bool,
    pub minor: bool,
    pub new_page: bool,
    pub patrolled: bool,
    pub timestamp_ms: i64,
    pub comment: Option<String>,
    pub length_old: i64,
    pub length_new: i64,
}

impl StreamEvent {
    pub fn byte_delta(&self) -> i32 {
        (self.length_new - self.length_old) as i32
    }
}

/// Parse a JSON event from the EventStreams SSE `data:` line.
pub fn parse_stream_event(json: &str) -> Option<StreamEvent> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;

    // Only process edits (not log, categorize, etc.)
    let change_type = v.get("type")?.as_str()?;
    if change_type != "edit" && change_type != "new" {
        return None;
    }

    Some(StreamEvent {
        wiki: v.get("wiki")?.as_str()?.to_string(),
        title: v.get("title")?.as_str()?.to_string(),
        namespace: v.get("namespace")?.as_i64()? as i32,
        rev_id: v.get("revision")?.get("new")?.as_u64()?,
        old_rev_id: v.get("revision")?.get("old")?.as_u64(),
        user: v.get("user")?.as_str()?.to_string(),
        bot: v.get("bot").and_then(|b| b.as_bool()).unwrap_or(false),
        minor: v.get("minor").and_then(|b| b.as_bool()).unwrap_or(false),
        new_page: change_type == "new",
        patrolled: v
            .get("patrolled")
            .and_then(|b| b.as_bool())
            .unwrap_or(false),
        timestamp_ms: v
            .get("timestamp")
            .and_then(|t| t.as_i64())
            .unwrap_or(0)
            * 1000,
        comment: v
            .get("comment")
            .and_then(|c| c.as_str())
            .filter(|s| !s.is_empty())
            .map(ToString::to_string),
        length_old: v
            .get("length")
            .and_then(|l| l.get("old"))
            .and_then(|n| n.as_i64())
            .unwrap_or(0),
        length_new: v
            .get("length")
            .and_then(|l| l.get("new"))
            .and_then(|n| n.as_i64())
            .unwrap_or(0),
    })
}

/// Start a browser EventSource connection to Wikimedia EventStreams.
/// Calls `on_event` for each matching edit on the target wiki.
#[cfg(target_arch = "wasm32")]
pub fn start_eventstream(wiki_id: &str, on_event: impl Fn(StreamEvent) + 'static) {
    use wasm_bindgen::closure::Closure;
    use web_sys::EventSource;

    let url = "https://stream.wikimedia.org/v2/stream/recentchange";
    let es = EventSource::new(url).expect("EventSource should construct");
    let wiki = wiki_id.to_string();

    let callback = Closure::wrap(Box::new(move |event: web_sys::MessageEvent| {
        let data = event.data().as_string().unwrap_or_default();
        if let Some(parsed) = parse_stream_event(&data) {
            if parsed.wiki == wiki {
                on_event(parsed);
            }
        }
    }) as Box<dyn Fn(web_sys::MessageEvent)>);

    es.set_onmessage(Some(callback.as_ref().unchecked_ref()));
    callback.forget(); // Leak the closure so it lives as long as the EventSource
    std::mem::forget(es); // Keep the EventSource alive
}

#[cfg(not(target_arch = "wasm32"))]
pub fn start_eventstream(_wiki_id: &str, _on_event: impl Fn(StreamEvent) + 'static) {}

#[cfg(test)]
mod tests {
    use super::parse_stream_event;

    #[test]
    fn parses_edit_event() {
        let json = r#"{
            "type": "edit",
            "wiki": "frwiki",
            "title": "Test",
            "namespace": 0,
            "revision": {"new": 123, "old": 122},
            "user": "Alice",
            "bot": false,
            "minor": false,
            "patrolled": false,
            "timestamp": 1700000000,
            "comment": "test edit",
            "length": {"old": 100, "new": 110}
        }"#;

        let ev = parse_stream_event(json).expect("should parse");
        assert_eq!(ev.wiki, "frwiki");
        assert_eq!(ev.rev_id, 123);
        assert_eq!(ev.byte_delta(), 10);
    }

    #[test]
    fn ignores_log_events() {
        let json = r#"{"type": "log", "wiki": "frwiki"}"#;
        assert!(parse_stream_event(json).is_none());
    }
}
