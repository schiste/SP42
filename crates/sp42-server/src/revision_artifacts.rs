use std::collections::HashMap;
use std::time::Duration;

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use sp42_core::{
    ContentModel, QueuedEdit, RenderedHunkPreview, RenderedHunkSide, StructuredDiff, WikiConfig,
    build_media_diff, diff_lines,
};
use tracing::warn;

use crate::action_routes::truncate_response_body;
use crate::http_errors::{gateway_error, unauthorized_error};
use crate::state::AppState;
use crate::{access_token_for_request, config_for_state_wiki};

const REVISION_ARTIFACT_CACHE_TTL_MS: i64 = 5 * 60 * 1000;
const RENDERED_HUNK_CACHE_TTL_MS: i64 = 5 * 60 * 1000;
const WIKIMEDIA_API_RETRY_ATTEMPTS: usize = 3;
const WIKIMEDIA_API_RETRY_DELAY_MS: u64 = 150;

/// A revision slot's content and its associated metadata.
/// The `content_model` field is populated by ADR-0016 D1 but routed on in a
/// future phase (D4) when content-model-specific handling is implemented.
#[derive(Debug, Clone)]
pub(crate) struct RevisionSlotContent {
    pub(crate) text: String,
    #[allow(dead_code)] // Phase 6: docs/platform/adr/0016-wikidata-entity-content-model.md #16
    pub(crate) content_model: ContentModel,
}

#[derive(Debug, Clone)]
pub(crate) struct RevisionArtifacts {
    pub(crate) diff: StructuredDiff,
    pub(crate) media_diff: Option<sp42_core::MediaDiffReport>,
}

#[derive(Debug, Clone)]
pub(crate) struct CachedRevisionArtifacts {
    pub(crate) fetched_at_ms: i64,
    pub(crate) artifacts: RevisionArtifacts,
}

#[derive(Debug, Clone)]
pub(crate) struct CachedRenderedHunkPreview {
    pub(crate) fetched_at_ms: i64,
    pub(crate) preview: RenderedHunkPreview,
}

pub(crate) async fn get_revision_diff(
    Path((wiki_id, rev_id, old_rev_id)): Path<(String, u64, u64)>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Option<sp42_core::StructuredDiff>>, (StatusCode, Json<serde_json::Value>)> {
    let access_token = access_token_for_request(&state, &headers)
        .await
        .ok_or_else(|| unauthorized_error("No authenticated Wikimedia session is active."))?;
    let config = config_for_state_wiki(&state, &wiki_id)?;
    let diff = fetch_revision_diff_by_ids(&state, &access_token, &config, rev_id, old_rev_id)
        .await
        .map_err(|error| {
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": error})),
            )
        })?;
    Ok(Json(diff))
}

pub(crate) async fn get_revision_media_diff(
    Path((wiki_id, rev_id, old_rev_id)): Path<(String, u64, u64)>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Option<sp42_core::MediaDiffReport>>, (StatusCode, Json<serde_json::Value>)> {
    let access_token = access_token_for_request(&state, &headers)
        .await
        .ok_or_else(|| unauthorized_error("No authenticated Wikimedia session is active."))?;
    let config = config_for_state_wiki(&state, &wiki_id)?;
    let report =
        fetch_revision_media_diff_by_ids(&state, &access_token, &config, rev_id, old_rev_id)
            .await
            .map_err(|error| {
                (
                    StatusCode::BAD_GATEWAY,
                    Json(serde_json::json!({"error": error})),
                )
            })?;
    Ok(Json(report))
}

pub(crate) async fn get_rendered_hunk_preview(
    Path((wiki_id, rev_id, old_rev_id, hunk_index)): Path<(String, u64, u64, usize)>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Option<RenderedHunkPreview>>, (StatusCode, Json<serde_json::Value>)> {
    let access_token = access_token_for_request(&state, &headers)
        .await
        .ok_or_else(|| unauthorized_error("No authenticated Wikimedia session is active."))?;
    let config = config_for_state_wiki(&state, &wiki_id)?;
    let preview = fetch_rendered_hunk_preview_by_ids(
        &state,
        &access_token,
        &config,
        rev_id,
        old_rev_id,
        hunk_index,
    )
    .await
    .map_err(gateway_error)?;
    Ok(Json(preview))
}

pub(crate) async fn fetch_revision_diff(
    state: &AppState,
    access_token: &str,
    config: &WikiConfig,
    item: &QueuedEdit,
) -> Result<Option<sp42_core::StructuredDiff>, String> {
    let Some(old_rev_id) = item.event.old_rev_id else {
        return Ok(None);
    };

    fetch_revision_diff_by_ids(state, access_token, config, item.event.rev_id, old_rev_id).await
}

pub(crate) async fn fetch_revision_media_diff(
    state: &AppState,
    access_token: &str,
    config: &WikiConfig,
    item: &QueuedEdit,
) -> Result<Option<sp42_core::MediaDiffReport>, String> {
    let Some(old_rev_id) = item.event.old_rev_id else {
        return Ok(None);
    };

    fetch_revision_media_diff_by_ids(state, access_token, config, item.event.rev_id, old_rev_id)
        .await
}

pub(crate) async fn fetch_revision_diff_by_ids(
    state: &AppState,
    access_token: &str,
    config: &WikiConfig,
    rev_id: u64,
    old_rev_id: u64,
) -> Result<Option<sp42_core::StructuredDiff>, String> {
    revision_artifacts_for_pair(state, access_token, config, rev_id, old_rev_id)
        .await
        .map(|artifacts| artifacts.map(|artifacts| artifacts.diff))
}

pub(crate) async fn fetch_revision_media_diff_by_ids(
    state: &AppState,
    access_token: &str,
    config: &WikiConfig,
    rev_id: u64,
    old_rev_id: u64,
) -> Result<Option<sp42_core::MediaDiffReport>, String> {
    revision_artifacts_for_pair(state, access_token, config, rev_id, old_rev_id)
        .await
        .map(|artifacts| artifacts.and_then(|artifacts| artifacts.media_diff))
}

pub(crate) async fn fetch_rendered_hunk_preview_by_ids(
    state: &AppState,
    access_token: &str,
    config: &WikiConfig,
    rev_id: u64,
    old_rev_id: u64,
    hunk_index: usize,
) -> Result<Option<RenderedHunkPreview>, String> {
    let cache_key = rendered_hunk_cache_key(&config.wiki_id, rev_id, old_rev_id, hunk_index);
    let now_ms = state.clock.now_ms();
    {
        let guard = state.rendered_hunks.read().await;
        if let Some(cached) = guard.get(&cache_key)
            && now_ms.saturating_sub(cached.fetched_at_ms) < RENDERED_HUNK_CACHE_TTL_MS
        {
            return Ok(Some(cached.preview.clone()));
        }
    }

    let Some(artifacts) =
        revision_artifacts_for_pair(state, access_token, config, rev_id, old_rev_id).await?
    else {
        return Ok(None);
    };
    let Some(hunk) = artifacts.diff.hunks.get(hunk_index) else {
        return Ok(None);
    };

    let before = render_revision_section_side(
        &state.http_client,
        access_token,
        config,
        old_rev_id,
        hunk.section.before.as_deref(),
    )
    .await?;
    let after = render_revision_section_side(
        &state.http_client,
        access_token,
        config,
        rev_id,
        hunk.section.after.as_deref(),
    )
    .await?;

    let mut warnings = Vec::new();
    if before.missing {
        warnings.push(format!(
            "Before revision section \"{}\" could not be rendered directly; the article structure may have changed.",
            before.section_label
        ));
    }
    if after.missing {
        warnings.push(format!(
            "After revision section \"{}\" could not be rendered directly; the article structure may have changed.",
            after.section_label
        ));
    }
    warnings.push(
        "Rendered preview is section-scoped and may omit cross-section references or context-sensitive template output."
            .to_string(),
    );

    let preview = RenderedHunkPreview {
        hunk_index,
        before,
        after,
        warnings,
    };

    let mut guard = state.rendered_hunks.write().await;
    guard.insert(
        cache_key,
        CachedRenderedHunkPreview {
            fetched_at_ms: now_ms,
            preview: preview.clone(),
        },
    );

    Ok(Some(preview))
}

async fn revision_artifacts_for_pair(
    state: &AppState,
    access_token: &str,
    config: &WikiConfig,
    rev_id: u64,
    old_rev_id: u64,
) -> Result<Option<RevisionArtifacts>, String> {
    let cache_key = revision_artifact_cache_key(&config.wiki_id, rev_id, old_rev_id);
    let now_ms = state.clock.now_ms();
    {
        let guard = state.revision_artifacts.read().await;
        if let Some(cached) = guard.get(&cache_key)
            && now_ms.saturating_sub(cached.fetched_at_ms) < REVISION_ARTIFACT_CACHE_TTL_MS
        {
            return Ok(Some(cached.artifacts.clone()));
        }
    }

    let Some((before_content, after_content)) =
        fetch_revision_text_pair(&state.http_client, access_token, config, rev_id, old_rev_id)
            .await?
    else {
        return Ok(None);
    };

    let diff = diff_lines(&before_content.text, &after_content.text);
    let media_diff = {
        let mut report = build_media_diff(&before_content.text, &after_content.text);
        if report.has_changes() {
            populate_media_preview_urls(config, &mut report);
            Some(report)
        } else {
            None
        }
    };
    let artifacts = RevisionArtifacts { diff, media_diff };

    let mut guard = state.revision_artifacts.write().await;
    guard.insert(
        cache_key,
        CachedRevisionArtifacts {
            fetched_at_ms: now_ms,
            artifacts: artifacts.clone(),
        },
    );

    Ok(Some(artifacts))
}

async fn fetch_revision_text_pair(
    client: &reqwest::Client,
    access_token: &str,
    config: &WikiConfig,
    rev_id: u64,
    old_rev_id: u64,
) -> Result<Option<(RevisionSlotContent, RevisionSlotContent)>, String> {
    let revisions =
        fetch_revision_texts(client, access_token, config, &[old_rev_id, rev_id]).await?;
    let Some(before) = revisions.get(&old_rev_id) else {
        return Ok(None);
    };
    let Some(after) = revisions.get(&rev_id) else {
        return Ok(None);
    };

    Ok(Some((before.clone(), after.clone())))
}

fn revision_artifact_cache_key(wiki_id: &str, rev_id: u64, old_rev_id: u64) -> String {
    format!("{wiki_id}:{old_rev_id}:{rev_id}")
}

fn rendered_hunk_cache_key(
    wiki_id: &str,
    rev_id: u64,
    old_rev_id: u64,
    hunk_index: usize,
) -> String {
    format!("{wiki_id}:{old_rev_id}:{rev_id}:hunk:{hunk_index}")
}

async fn render_revision_section_side(
    client: &reqwest::Client,
    access_token: &str,
    config: &WikiConfig,
    rev_id: u64,
    section_label: Option<&str>,
) -> Result<RenderedHunkSide, String> {
    let label = section_label.unwrap_or("Lead").trim();
    if label.is_empty() || label == "Lead" {
        let html = fetch_rendered_section_html(client, access_token, config, rev_id, 0).await?;
        return Ok(RenderedHunkSide {
            section_label: "Lead".to_string(),
            html,
            missing: false,
        });
    }

    let sections = fetch_revision_sections(client, access_token, config, rev_id).await?;
    let Some(section_index) = sections
        .into_iter()
        .find_map(|(candidate_label, index)| (candidate_label == label).then_some(index))
    else {
        return Ok(RenderedHunkSide {
            section_label: label.to_string(),
            html: String::new(),
            missing: true,
        });
    };

    let html =
        fetch_rendered_section_html(client, access_token, config, rev_id, section_index).await?;
    Ok(RenderedHunkSide {
        section_label: label.to_string(),
        html,
        missing: false,
    })
}

async fn fetch_revision_sections(
    client: &reqwest::Client,
    access_token: &str,
    config: &WikiConfig,
    rev_id: u64,
) -> Result<Vec<(String, u32)>, String> {
    let oldid = rev_id.to_string();
    let body = fetch_wikimedia_api_bytes(
        client,
        access_token,
        config,
        &[
            ("action", "parse"),
            ("oldid", oldid.as_str()),
            ("prop", "sections"),
            ("format", "json"),
            ("formatversion", "2"),
        ],
        "section lookup",
    )
    .await?;

    let value: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|error| format!("section lookup JSON failed: {error}"))?;
    let sections = value
        .get("parse")
        .and_then(|value| value.get("sections"))
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "section lookup payload does not contain parse.sections".to_string())?;

    Ok(sections
        .iter()
        .filter_map(|section| {
            let label = section.get("line")?.as_str()?.trim();
            let index = section.get("index")?;
            let index = index
                .as_u64()
                .or_else(|| index.as_str().and_then(|text| text.parse::<u64>().ok()))?;
            let index = u32::try_from(index).ok()?;
            Some((label.to_string(), index))
        })
        .collect())
}

async fn fetch_rendered_section_html(
    client: &reqwest::Client,
    access_token: &str,
    config: &WikiConfig,
    rev_id: u64,
    section_index: u32,
) -> Result<String, String> {
    let oldid = rev_id.to_string();
    let section = section_index.to_string();
    let body = fetch_wikimedia_api_bytes(
        client,
        access_token,
        config,
        &[
            ("action", "parse"),
            ("oldid", oldid.as_str()),
            ("prop", "text"),
            ("section", section.as_str()),
            ("format", "json"),
            ("formatversion", "2"),
        ],
        "rendered section",
    )
    .await?;

    let value: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|error| format!("rendered section JSON failed: {error}"))?;
    extract_parse_html(&value)
}

fn extract_parse_html(value: &serde_json::Value) -> Result<String, String> {
    let text = value
        .get("parse")
        .and_then(|value| value.get("text"))
        .ok_or_else(|| "rendered section payload does not contain parse.text".to_string())?;

    if let Some(html) = text.as_str() {
        return Ok(html.to_string());
    }

    if let Some(object) = text.as_object()
        && let Some(html) = object.get("*").and_then(serde_json::Value::as_str)
    {
        return Ok(html.to_string());
    }

    Err("rendered section payload does not expose HTML text".to_string())
}

fn populate_media_preview_urls(config: &WikiConfig, report: &mut sp42_core::MediaDiffReport) {
    for entry in &mut report.entries {
        entry.page_url = build_file_page_url(config, &entry.file_name);
        entry.preview_url = build_file_preview_url(config, &entry.display_title);
    }
}

fn build_file_page_url(config: &WikiConfig, file_name: &str) -> Option<url::Url> {
    let mut url = wiki_origin_url(config);
    {
        let mut segments = url.path_segments_mut().ok()?;
        segments.push("wiki");
        segments.push(file_name);
    }
    Some(url)
}

fn build_file_preview_url(config: &WikiConfig, display_title: &str) -> Option<url::Url> {
    let mut url = wiki_origin_url(config);
    {
        let mut segments = url.path_segments_mut().ok()?;
        segments.push("wiki");
        segments.push("Special:Redirect");
        segments.push("file");
        segments.push(display_title);
    }
    url.query_pairs_mut().append_pair("width", "320");
    Some(url)
}

fn wiki_origin_url(config: &WikiConfig) -> url::Url {
    let mut url = config.api_url.clone();
    url.set_path("");
    url.set_query(None);
    url.set_fragment(None);
    url
}

/// Parse a revision slot object from Wikimedia API response into `RevisionSlotContent`.
/// Extracts the text content and content model, defaulting to `ContentModel::Wikitext`
/// when contentmodel is absent from the response.
fn parse_revision_slot_to_content(
    main_slot: &serde_json::Map<String, serde_json::Value>,
) -> Option<RevisionSlotContent> {
    let content = main_slot
        .get("content")
        .and_then(serde_json::Value::as_str)
        .map(ToString::to_string)?;
    let content_model = main_slot
        .get("contentmodel")
        .and_then(serde_json::Value::as_str)
        .map(ContentModel::parse)
        .unwrap_or_default();
    Some(RevisionSlotContent {
        text: content,
        content_model,
    })
}

async fn fetch_revision_texts(
    client: &reqwest::Client,
    access_token: &str,
    config: &WikiConfig,
    revision_ids: &[u64],
) -> Result<HashMap<u64, RevisionSlotContent>, String> {
    if revision_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let revids = revision_ids
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("|");
    let body = fetch_wikimedia_api_bytes(
        client,
        access_token,
        config,
        &[
            ("action", "query"),
            ("prop", "revisions"),
            ("revids", revids.as_str()),
            ("rvprop", "ids|content|contentmodel"),
            ("rvslots", "main"),
            ("format", "json"),
            ("formatversion", "2"),
        ],
        "revision lookup",
    )
    .await?;

    let parsed: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|error| format!("revision lookup JSON failed: {error}"))?;
    let mut map = HashMap::new();
    let pages = parsed
        .get("query")
        .and_then(|value| value.get("pages"))
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "revision lookup payload does not contain query.pages".to_string())?;

    for page in pages {
        let Some(revisions) = page.get("revisions").and_then(serde_json::Value::as_array) else {
            continue;
        };
        for revision in revisions {
            let Some(rev_id) = revision.get("revid").and_then(serde_json::Value::as_u64) else {
                continue;
            };
            let Some(main_slot) = revision
                .get("slots")
                .and_then(|value| value.get("main"))
                .and_then(serde_json::Value::as_object)
            else {
                continue;
            };
            if let Some(slot_content) = parse_revision_slot_to_content(main_slot) {
                map.insert(rev_id, slot_content);
            }
        }
    }

    Ok(map)
}

async fn fetch_wikimedia_api_bytes(
    client: &reqwest::Client,
    access_token: &str,
    config: &WikiConfig,
    query: &[(&str, &str)],
    label: &str,
) -> Result<Vec<u8>, String> {
    let mut last_error = None;

    for attempt in 1..=WIKIMEDIA_API_RETRY_ATTEMPTS {
        let response = client
            .get(config.api_url.clone())
            .bearer_auth(access_token)
            .query(query)
            .send()
            .await;

        match response {
            Ok(response) => {
                let status = response.status();
                let body = response
                    .bytes()
                    .await
                    .map_err(|error| format!("{label} body failed: {error}"))?
                    .to_vec();

                if status.is_success() {
                    return Ok(body);
                }

                let retryable = matches!(
                    status,
                    StatusCode::TOO_MANY_REQUESTS
                        | StatusCode::BAD_GATEWAY
                        | StatusCode::SERVICE_UNAVAILABLE
                        | StatusCode::GATEWAY_TIMEOUT
                );
                let message = format!(
                    "{label} failed with HTTP {} on attempt {attempt}/{}: {}",
                    status.as_u16(),
                    WIKIMEDIA_API_RETRY_ATTEMPTS,
                    truncate_response_body(&body)
                );
                if retryable && attempt < WIKIMEDIA_API_RETRY_ATTEMPTS {
                    warn!(
                        label,
                        attempt,
                        status = status.as_u16(),
                        "retrying wikimedia api request"
                    );
                    tokio::time::sleep(Duration::from_millis(WIKIMEDIA_API_RETRY_DELAY_MS)).await;
                    last_error = Some(message);
                    continue;
                }
                return Err(message);
            }
            Err(error) => {
                let message = format!(
                    "{label} transport failed on attempt {attempt}/{WIKIMEDIA_API_RETRY_ATTEMPTS}: {error}"
                );
                if attempt < WIKIMEDIA_API_RETRY_ATTEMPTS {
                    warn!(label, attempt, error = %error, "retrying wikimedia api transport failure");
                    tokio::time::sleep(Duration::from_millis(WIKIMEDIA_API_RETRY_DELAY_MS)).await;
                    last_error = Some(message);
                    continue;
                }
                return Err(message);
            }
        }
    }

    Err(last_error.unwrap_or_else(|| format!("{label} failed without a response")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revision_slot_content_parses_contentmodel_variants() {
        // Verify that parse_revision_slot_to_content correctly handles contentmodel
        // variants (ADR-0016 D1) and defaults to Wikitext when contentmodel is absent.

        // Test case: wikitext slot (typical Wikipedia edit).
        let wikitext_slot = serde_json::json!({
            "content": "Example wikitext",
            "contentmodel": "wikitext"
        });
        let result = parse_revision_slot_to_content(wikitext_slot.as_object().unwrap());
        let slot = result.expect("wikitext slot should parse");
        assert_eq!(slot.text, "Example wikitext");
        assert_eq!(slot.content_model, ContentModel::Wikitext);

        // Test case: wikibase-item slot (Wikidata item).
        let wikibase_item_slot = serde_json::json!({
            "content": "{}",
            "contentmodel": "wikibase-item"
        });
        let result = parse_revision_slot_to_content(wikibase_item_slot.as_object().unwrap());
        let slot = result.expect("wikibase-item slot should parse");
        assert_eq!(slot.text, "{}");
        assert_eq!(slot.content_model, ContentModel::WikibaseItem);

        // Test case: contentmodel absent (should default to wikitext).
        let no_contentmodel_slot = serde_json::json!({
            "content": "Some text"
        });
        let result = parse_revision_slot_to_content(no_contentmodel_slot.as_object().unwrap());
        let slot = result.expect("slot without contentmodel should parse");
        assert_eq!(slot.text, "Some text");
        assert_eq!(
            slot.content_model,
            ContentModel::Wikitext,
            "contentmodel defaults to wikitext when absent from the response"
        );

        // Test case: missing content should return None.
        let no_content_slot = serde_json::json!({
            "contentmodel": "wikitext"
        });
        let result = parse_revision_slot_to_content(no_content_slot.as_object().unwrap());
        assert!(result.is_none(), "slot without content should return None");
    }
}
