use std::collections::BTreeMap;

use serde_json::Value;
use sp42_types::{HttpMethod, HttpRequest};
use url::Url;

use super::model::EntityId;
use super::parse::WikibaseParseError;

/// Keyless entity read, optionally pinned to a revision (`Special:EntityData?revision=`).
/// `revision: None` = current (what PR #103's verb uses); `Some(rev)` = e.g. the parent, for a diff.
/// Targets Wikidata.org; test instances arrive with patrol wiring.
///
/// # Panics
///
/// Panics if the URL format is invalid (should never happen with fixed Wikidata.org URLs).
#[must_use]
pub fn build_entity_request(id: &EntityId, revision: Option<u64>) -> HttpRequest {
    let base_url = format!(
        "https://www.wikidata.org/wiki/Special:EntityData/{}.json",
        id.as_str()
    );

    let url = if let Some(rev) = revision {
        Url::parse(&format!("{base_url}?revision={rev}"))
            .unwrap_or_else(|_| Url::parse(&base_url).expect("base URL should parse"))
    } else {
        Url::parse(&base_url).expect("base URL should parse")
    };

    HttpRequest {
        method: HttpMethod::Get,
        url,
        headers: BTreeMap::new(),
        body: Vec::new(),
    }
}

/// `wbgetentities&props=labels` — resolve property/item ids to display labels.
/// Targets Wikidata.org; test instances arrive with patrol wiring.
///
/// # Panics
///
/// Panics if the URL cannot be constructed (should never happen with valid params).
#[must_use]
pub fn build_label_request(ids: &[&str], lang: &str) -> HttpRequest {
    let ids_joined = ids.join("|");
    let url = Url::parse_with_params(
        "https://www.wikidata.org/w/api.php",
        &[
            ("action", "wbgetentities"),
            ("ids", ids_joined.as_str()),
            ("props", "labels"),
            ("languages", lang),
            ("format", "json"),
        ],
    )
    .expect("URL and params should be valid");

    HttpRequest {
        method: HttpMethod::Get,
        url,
        headers: BTreeMap::new(),
        body: Vec::new(),
    }
}

/// Labels keyed by entity/property id, in the requested language.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Labels(BTreeMap<String, String>);

impl Labels {
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&str> {
        self.0.get(id).map(String::as_str)
    }
}

/// Parse labels from a Wikibase API response (`wbgetentities&props=labels`).
///
/// # Errors
///
/// Returns `WikibaseParseError::InvalidJson` if the body is not valid JSON.
pub fn parse_labels(body: &[u8]) -> Result<Labels, WikibaseParseError> {
    let value: Value = serde_json::from_slice(body)?;

    let mut labels = BTreeMap::new();

    if let Some(entities) = value.get("entities").and_then(|v| v.as_object()) {
        for (id, entity_obj) in entities {
            if let Some(labels_obj) = entity_obj.get("labels").and_then(|v| v.as_object()) {
                // Get the first (only) language's value
                if let Some((_, lang_obj)) = labels_obj.iter().next()
                    && let Some(label_value) = lang_obj.get("value").and_then(|v| v.as_str())
                {
                    labels.insert(id.clone(), label_value.to_string());
                }
            }
        }
    }

    Ok(Labels(labels))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wikibase::EntityId;

    #[test]
    fn entity_request_targets_entitydata_current() {
        let req = build_entity_request(&EntityId::new("Q42"), None);
        assert_eq!(
            req.url.as_str(),
            "https://www.wikidata.org/wiki/Special:EntityData/Q42.json"
        );
    }

    #[test]
    fn entity_request_pins_a_revision() {
        let req = build_entity_request(&EntityId::new("Q42"), Some(2_000_200));
        assert_eq!(
            req.url.as_str(),
            "https://www.wikidata.org/wiki/Special:EntityData/Q42.json?revision=2000200"
        );
    }

    #[test]
    fn label_request_batches_ids() {
        let req = build_label_request(&["P69", "Q691283"], "en");
        let url = req.url.as_str();
        assert!(url.starts_with("https://www.wikidata.org/w/api.php?"));
        for needle in [
            "action=wbgetentities",
            "ids=P69%7CQ691283",
            "props=labels",
            "languages=en",
            "format=json",
        ] {
            assert!(url.contains(needle), "missing {needle} in {url}");
        }
    }

    #[test]
    fn parses_labels() {
        let labels =
            parse_labels(include_str!("../../../../fixtures/wikibase/q42_labels.json").as_bytes())
                .expect("parses");
        assert_eq!(labels.get("P69"), Some("educated at"));
        assert_eq!(labels.get("Q691283"), Some("St John's College"));
        assert_eq!(labels.get("Q1"), None);
    }
}
