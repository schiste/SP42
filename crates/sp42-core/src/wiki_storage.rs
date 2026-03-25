//! Shared conventions for storing SP42 public state on-wiki.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

use crate::branding::PROJECT_NAME;
use crate::errors::WikiStorageError;
use crate::traits::HttpClient;
use crate::types::{FlagState, HttpMethod, HttpRequest, HttpResponse, WikiConfig};
use crate::{ActionError, WikiPageSaveRequest, execute_wiki_page_save};

const PAYLOAD_BEGIN_MARKER: &str = "<!-- SP42:BEGIN -->";
const PAYLOAD_END_MARKER: &str = "<!-- SP42:END -->";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WikiStorageConfig {
    pub project_root_segment: String,
    pub personal_namespace: String,
    pub shared_namespace: String,
    pub meta_wiki_id: String,
}

impl Default for WikiStorageConfig {
    fn default() -> Self {
        Self {
            project_root_segment: PROJECT_NAME.to_string(),
            personal_namespace: "User".to_string(),
            shared_namespace: "User".to_string(),
            meta_wiki_id: "metawiki".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WikiStoragePlanInput {
    pub username: String,
    pub home_wiki_id: String,
    pub target_wiki_id: String,
    pub shared_owner_username: String,
    #[serde(default)]
    pub team_slugs: Vec<String>,
    #[serde(default)]
    pub rule_set_slugs: Vec<String>,
    #[serde(default)]
    pub training_dataset_slugs: Vec<String>,
    #[serde(default)]
    pub audit_period_slugs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WikiStorageRealm {
    PersonalUserSpace,
    SharedMetaUserSpace,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WikiStorageDocumentKind {
    PersonalIndex,
    PersonalProfile,
    PersonalPreferences,
    PersonalQueue {
        wiki_id: String,
    },
    PersonalWorkspace {
        wiki_id: String,
    },
    PersonalLabels {
        wiki_id: String,
    },
    SharedRegistry {
        wiki_id: String,
    },
    SharedTeam {
        wiki_id: String,
        team_slug: String,
    },
    SharedRuleSet {
        wiki_id: String,
        rule_set_slug: String,
    },
    SharedTrainingDataset {
        wiki_id: String,
        dataset_slug: String,
    },
    SharedAuditPeriod {
        wiki_id: String,
        period_slug: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WikiStorageDocument {
    pub site_wiki_id: String,
    pub realm: WikiStorageRealm,
    pub kind: WikiStorageDocumentKind,
    pub title: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WikiStoragePlan {
    pub personal_root: WikiStorageDocument,
    pub personal_documents: Vec<WikiStorageDocument>,
    pub shared_root: WikiStorageDocument,
    pub shared_documents: Vec<WikiStorageDocument>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WikiStoragePayloadEnvelope {
    pub project: String,
    pub version: u32,
    pub title: String,
    pub kind: String,
    pub site_wiki_id: String,
    pub realm: WikiStorageRealm,
    pub data: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WikiStorageLoadedDocument {
    pub title: String,
    pub exists: bool,
    pub page_id: Option<u64>,
    pub revision_id: Option<u64>,
    pub body: Option<String>,
    pub envelope: Option<WikiStoragePayloadEnvelope>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WikiStorageWriteRequest {
    pub document: WikiStorageDocument,
    #[serde(default)]
    pub human_summary: Vec<String>,
    pub data: Value,
    pub token: String,
    pub baserevid: Option<u64>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub watchlist: Option<String>,
    pub create_only: FlagState,
    pub minor: FlagState,
    pub summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WikiStorageWriteOutcome {
    pub title: String,
    pub baserevid: Option<u64>,
    pub http_status: u16,
    pub result: Option<String>,
}

#[must_use]
pub fn build_wiki_storage_plan(
    config: &WikiStorageConfig,
    input: &WikiStoragePlanInput,
) -> WikiStoragePlan {
    let personal_root_title =
        build_personal_title(config, &input.username, std::iter::empty::<&str>());
    let personal_root = WikiStorageDocument {
        site_wiki_id: input.home_wiki_id.clone(),
        realm: WikiStorageRealm::PersonalUserSpace,
        kind: WikiStorageDocumentKind::PersonalIndex,
        title: personal_root_title,
        description: format!(
            "Personal SP42 landing page for {} on {}.",
            input.username, input.home_wiki_id
        ),
    };

    let personal_documents = build_personal_documents(config, input);

    let shared_root = WikiStorageDocument {
        site_wiki_id: config.meta_wiki_id.clone(),
        realm: WikiStorageRealm::SharedMetaUserSpace,
        kind: WikiStorageDocumentKind::SharedRegistry {
            wiki_id: input.target_wiki_id.clone(),
        },
        title: build_shared_title(
            config,
            &input.shared_owner_username,
            [&input.target_wiki_id, "Registry"],
        ),
        description: format!(
            "Shared SP42 registry and landing page for {} on Meta-Wiki.",
            input.target_wiki_id
        ),
    };

    let shared_documents = build_shared_documents(config, input);
    let notes = vec![
        format!(
            "Personal durable state lives on {} under {}.",
            input.home_wiki_id, personal_root.title
        ),
        format!(
            "Shared durable state lives on {} under {}.",
            config.meta_wiki_id, shared_root.title
        ),
        "Realtime coordination, sessions, tokens, and in-flight action state remain memory-only."
            .to_string(),
    ];

    WikiStoragePlan {
        personal_root,
        personal_documents,
        shared_root,
        shared_documents,
        notes,
    }
}

fn build_personal_documents(
    config: &WikiStorageConfig,
    input: &WikiStoragePlanInput,
) -> Vec<WikiStorageDocument> {
    vec![
        WikiStorageDocument {
            site_wiki_id: input.home_wiki_id.clone(),
            realm: WikiStorageRealm::PersonalUserSpace,
            kind: WikiStorageDocumentKind::PersonalProfile,
            title: build_personal_title(config, &input.username, ["Profile"]),
            description: "Public operator identity, home wiki, and visible profile metadata."
                .to_string(),
        },
        WikiStorageDocument {
            site_wiki_id: input.home_wiki_id.clone(),
            realm: WikiStorageRealm::PersonalUserSpace,
            kind: WikiStorageDocumentKind::PersonalPreferences,
            title: build_personal_title(config, &input.username, ["Preferences"]),
            description: "User-visible patrol preferences, filters, and UI defaults.".to_string(),
        },
        WikiStorageDocument {
            site_wiki_id: input.home_wiki_id.clone(),
            realm: WikiStorageRealm::PersonalUserSpace,
            kind: WikiStorageDocumentKind::PersonalQueue {
                wiki_id: input.target_wiki_id.clone(),
            },
            title: build_personal_title(config, &input.username, ["Queues", &input.target_wiki_id]),
            description: format!(
                "Public queue preferences and saved queue state for {}.",
                input.target_wiki_id
            ),
        },
        WikiStorageDocument {
            site_wiki_id: input.home_wiki_id.clone(),
            realm: WikiStorageRealm::PersonalUserSpace,
            kind: WikiStorageDocumentKind::PersonalWorkspace {
                wiki_id: input.target_wiki_id.clone(),
            },
            title: build_personal_title(
                config,
                &input.username,
                ["Workspace", &input.target_wiki_id],
            ),
            description: format!(
                "Public working notes and saved review context for {}.",
                input.target_wiki_id
            ),
        },
        WikiStorageDocument {
            site_wiki_id: input.home_wiki_id.clone(),
            realm: WikiStorageRealm::PersonalUserSpace,
            kind: WikiStorageDocumentKind::PersonalLabels {
                wiki_id: input.target_wiki_id.clone(),
            },
            title: build_personal_title(config, &input.username, ["Labels", &input.target_wiki_id]),
            description: format!(
                "Public labels and training examples contributed by the user for {}.",
                input.target_wiki_id
            ),
        },
    ]
}

#[must_use]
pub fn render_wiki_storage_index_page(
    root: &WikiStorageDocument,
    linked_documents: &[WikiStorageDocument],
    summary_lines: &[String],
) -> String {
    let mut lines = vec![
        format!(
            "= {} =",
            root.title.rsplit('/').next().unwrap_or(PROJECT_NAME)
        ),
        String::new(),
        root.description.clone(),
        String::new(),
        "== Linked Pages ==".to_string(),
    ];

    for document in linked_documents {
        lines.push(format!(
            "* [[{}]]: {}",
            document.title, document.description
        ));
    }

    if !summary_lines.is_empty() {
        lines.push(String::new());
        lines.push("== Notes ==".to_string());
        for line in summary_lines {
            lines.push(format!("* {line}"));
        }
    }

    lines.join("\n")
}

/// Render a human-readable wiki page with an embedded machine JSON block.
///
/// # Errors
///
/// Returns [`WikiStorageError`] when the embedded JSON payload cannot be
/// serialized.
pub fn render_wiki_storage_document_page(
    document: &WikiStorageDocument,
    human_summary: &[String],
    data: &Value,
) -> Result<String, WikiStorageError> {
    let envelope = WikiStoragePayloadEnvelope {
        project: PROJECT_NAME.to_string(),
        version: 1,
        title: document.title.clone(),
        kind: document_kind_label(&document.kind).to_string(),
        site_wiki_id: document.site_wiki_id.clone(),
        realm: document.realm.clone(),
        data: data.clone(),
    };
    let payload =
        serde_json::to_string_pretty(&envelope).map_err(|error| WikiStorageError::Serialize {
            message: error.to_string(),
        })?;

    let mut lines = vec![
        format!(
            "= {} =",
            document.title.rsplit('/').next().unwrap_or(PROJECT_NAME)
        ),
        String::new(),
        document.description.clone(),
    ];

    if !human_summary.is_empty() {
        lines.push(String::new());
        lines.push("== Summary ==".to_string());
        for line in human_summary {
            lines.push(format!("* {line}"));
        }
    }

    lines.push(String::new());
    lines.push("== Machine payload ==".to_string());
    lines.push(PAYLOAD_BEGIN_MARKER.to_string());
    lines.push("<syntaxhighlight lang=\"json\">".to_string());
    lines.push(payload);
    lines.push("</syntaxhighlight>".to_string());
    lines.push(PAYLOAD_END_MARKER.to_string());

    Ok(lines.join("\n"))
}

/// Build a `MediaWiki` query request for reading a stored SP42 document.
///
/// # Errors
///
/// Returns [`WikiStorageError`] when the requested title is empty.
pub fn build_wiki_storage_document_load_request(
    config: &WikiConfig,
    title: &str,
) -> Result<HttpRequest, WikiStorageError> {
    if title.trim().is_empty() {
        return Err(WikiStorageError::InvalidInput {
            message: "title is required".to_string(),
        });
    }

    Ok(HttpRequest {
        method: HttpMethod::Get,
        url: build_query_url(
            &config.api_url,
            &[
                ("action", "query"),
                ("prop", "revisions"),
                ("titles", title),
                ("rvprop", "ids|content"),
                ("rvslots", "main"),
                ("format", "json"),
                ("formatversion", "2"),
            ],
        ),
        headers: BTreeMap::default(),
        body: Vec::new(),
    })
}

/// Load and parse a canonical SP42 on-wiki document through an injected HTTP client.
///
/// # Errors
///
/// Returns [`WikiStorageError`] when the request is invalid, transport fails,
/// or the response cannot be parsed into a canonical SP42 document.
pub async fn load_wiki_storage_document<C>(
    client: &C,
    config: &WikiConfig,
    title: &str,
) -> Result<WikiStorageLoadedDocument, WikiStorageError>
where
    C: HttpClient + ?Sized,
{
    let request = build_wiki_storage_document_load_request(config, title)?;
    let response = client
        .execute(request)
        .await
        .map_err(|error| WikiStorageError::Transport {
            message: error.to_string(),
        })?;
    parse_wiki_storage_document_response(title, &response)
}

/// Parse a `MediaWiki` page query response into a canonical SP42 document view.
///
/// # Errors
///
/// Returns [`WikiStorageError`] when the HTTP status is not successful, the
/// JSON body is invalid, or the embedded SP42 payload cannot be parsed.
pub fn parse_wiki_storage_document_response(
    requested_title: &str,
    response: &HttpResponse,
) -> Result<WikiStorageLoadedDocument, WikiStorageError> {
    if !(200..300).contains(&response.status) {
        return Err(WikiStorageError::Transport {
            message: format!(
                "wiki document fetch returned HTTP {} for `{requested_title}`",
                response.status
            ),
        });
    }

    let parsed: WikiStorageQueryResponse =
        serde_json::from_slice(&response.body).map_err(|error| WikiStorageError::Serialize {
            message: error.to_string(),
        })?;
    let page =
        parsed
            .query
            .pages
            .into_iter()
            .next()
            .ok_or_else(|| WikiStorageError::Transport {
                message: "wiki document query returned no pages".to_string(),
            })?;

    let body = page
        .revisions
        .as_ref()
        .and_then(|revisions| revisions.first())
        .and_then(|revision| revision.slots.main.content.clone());
    let envelope = body
        .as_deref()
        .map(parse_wiki_storage_payload_envelope)
        .transpose()?;

    Ok(WikiStorageLoadedDocument {
        title: page.title,
        exists: !page.missing.unwrap_or(false),
        page_id: page.pageid,
        revision_id: page
            .revisions
            .and_then(|revisions| revisions.first().map(|revision| revision.revid)),
        body,
        envelope,
    })
}

/// Extract the SP42 machine payload envelope embedded in a wiki page body.
///
/// # Errors
///
/// Returns [`WikiStorageError`] when payload markers are missing or the
/// embedded JSON block is invalid.
pub fn parse_wiki_storage_payload_envelope(
    body: &str,
) -> Result<WikiStoragePayloadEnvelope, WikiStorageError> {
    let begin = body
        .find(PAYLOAD_BEGIN_MARKER)
        .ok_or_else(|| WikiStorageError::InvalidInput {
            message: "document does not contain SP42 payload markers".to_string(),
        })?;
    let end = body
        .find(PAYLOAD_END_MARKER)
        .ok_or_else(|| WikiStorageError::InvalidInput {
            message: "document does not contain a closing SP42 payload marker".to_string(),
        })?;
    if end <= begin {
        return Err(WikiStorageError::InvalidInput {
            message: "document payload markers are malformed".to_string(),
        });
    }

    let payload_section = &body[begin + PAYLOAD_BEGIN_MARKER.len()..end];
    let payload = payload_section
        .replace("<syntaxhighlight lang=\"json\">", "")
        .replace("</syntaxhighlight>", "")
        .trim()
        .to_string();

    serde_json::from_str(&payload).map_err(|error| WikiStorageError::Serialize {
        message: error.to_string(),
    })
}

/// Render and save a canonical SP42 document to a wiki page.
///
/// # Errors
///
/// Returns [`WikiStorageError`] when rendering fails, the save transport fails,
/// or the underlying write is rejected because of a conflict.
pub async fn save_wiki_storage_document<C>(
    client: &C,
    config: &WikiConfig,
    request: &WikiStorageWriteRequest,
) -> Result<WikiStorageWriteOutcome, WikiStorageError>
where
    C: HttpClient + ?Sized,
{
    let text = render_wiki_storage_document_page(
        &request.document,
        &request.human_summary,
        &request.data,
    )?;
    let response = execute_wiki_page_save(
        client,
        config,
        &WikiPageSaveRequest {
            title: request.document.title.clone(),
            text,
            token: request.token.clone(),
            summary: request.summary.clone(),
            baserevid: request.baserevid,
            tags: request.tags.clone(),
            watchlist: request.watchlist.clone(),
            create_only: request.create_only,
            minor: request.minor,
        },
    )
    .await
    .map_err(|error| map_wiki_storage_write_error(&request.document.title, error))?;
    let summary =
        crate::parse_action_response_summary(&response, "page save").map_err(|error| {
            WikiStorageError::Transport {
                message: error.to_string(),
            }
        })?;

    Ok(WikiStorageWriteOutcome {
        title: request.document.title.clone(),
        baserevid: request.baserevid,
        http_status: response.status,
        result: summary.result,
    })
}

fn map_wiki_storage_write_error(title: &str, error: ActionError) -> WikiStorageError {
    match error {
        ActionError::Execution {
            message,
            code,
            http_status: _,
            retryable: _,
        } => {
            let is_conflict = matches!(
                code.as_deref(),
                Some("editconflict" | "articleexists" | "pagedeleted" | "missingtitle")
            );
            if is_conflict {
                WikiStorageError::Conflict {
                    title: title.to_string(),
                    message,
                }
            } else {
                WikiStorageError::Transport { message }
            }
        }
    }
}

fn build_query_url(base_url: &Url, params: &[(&str, &str)]) -> Url {
    let mut url = base_url.clone();
    {
        let mut query = url.query_pairs_mut();
        query.clear().extend_pairs(params.iter().copied());
    }
    url
}

#[derive(Debug, Deserialize)]
struct WikiStorageQueryResponse {
    query: WikiStorageQueryPages,
}

#[derive(Debug, Deserialize)]
struct WikiStorageQueryPages {
    pages: Vec<WikiStoragePageRecord>,
}

#[derive(Debug, Deserialize)]
struct WikiStoragePageRecord {
    title: String,
    pageid: Option<u64>,
    missing: Option<bool>,
    revisions: Option<Vec<WikiStorageRevisionRecord>>,
}

#[derive(Debug, Deserialize)]
struct WikiStorageRevisionRecord {
    revid: u64,
    slots: WikiStorageRevisionSlots,
}

#[derive(Debug, Deserialize)]
struct WikiStorageRevisionSlots {
    main: WikiStorageRevisionSlotMain,
}

#[derive(Debug, Deserialize)]
struct WikiStorageRevisionSlotMain {
    #[serde(rename = "content")]
    content: Option<String>,
}

fn build_shared_documents(
    config: &WikiStorageConfig,
    input: &WikiStoragePlanInput,
) -> Vec<WikiStorageDocument> {
    let team_slugs = default_if_empty(&input.team_slugs, "core");
    let rule_set_slugs = default_if_empty(&input.rule_set_slugs, "default");
    let training_dataset_slugs = default_if_empty(&input.training_dataset_slugs, "main");
    let audit_period_slugs = default_if_empty(&input.audit_period_slugs, "current");

    let mut documents = Vec::new();

    for team_slug in team_slugs {
        documents.push(WikiStorageDocument {
            site_wiki_id: config.meta_wiki_id.clone(),
            realm: WikiStorageRealm::SharedMetaUserSpace,
            kind: WikiStorageDocumentKind::SharedTeam {
                wiki_id: input.target_wiki_id.clone(),
                team_slug: team_slug.clone(),
            },
            title: build_shared_title(
                config,
                &input.shared_owner_username,
                [&input.target_wiki_id, "Teams", team_slug.as_str()],
            ),
            description: format!(
                "Shared team definition `{team_slug}` for {}.",
                input.target_wiki_id
            ),
        });
    }

    for rule_set_slug in rule_set_slugs {
        documents.push(WikiStorageDocument {
            site_wiki_id: config.meta_wiki_id.clone(),
            realm: WikiStorageRealm::SharedMetaUserSpace,
            kind: WikiStorageDocumentKind::SharedRuleSet {
                wiki_id: input.target_wiki_id.clone(),
                rule_set_slug: rule_set_slug.clone(),
            },
            title: build_shared_title(
                config,
                &input.shared_owner_username,
                [&input.target_wiki_id, "Rules", rule_set_slug.as_str()],
            ),
            description: format!(
                "Shared rule set `{rule_set_slug}` for {}.",
                input.target_wiki_id
            ),
        });
    }

    for dataset_slug in training_dataset_slugs {
        documents.push(WikiStorageDocument {
            site_wiki_id: config.meta_wiki_id.clone(),
            realm: WikiStorageRealm::SharedMetaUserSpace,
            kind: WikiStorageDocumentKind::SharedTrainingDataset {
                wiki_id: input.target_wiki_id.clone(),
                dataset_slug: dataset_slug.clone(),
            },
            title: build_shared_title(
                config,
                &input.shared_owner_username,
                [&input.target_wiki_id, "Training", dataset_slug.as_str()],
            ),
            description: format!(
                "Shared public training dataset `{dataset_slug}` for {}.",
                input.target_wiki_id
            ),
        });
    }

    for period_slug in audit_period_slugs {
        documents.push(WikiStorageDocument {
            site_wiki_id: config.meta_wiki_id.clone(),
            realm: WikiStorageRealm::SharedMetaUserSpace,
            kind: WikiStorageDocumentKind::SharedAuditPeriod {
                wiki_id: input.target_wiki_id.clone(),
                period_slug: period_slug.clone(),
            },
            title: build_shared_title(
                config,
                &input.shared_owner_username,
                [&input.target_wiki_id, "Audit", period_slug.as_str()],
            ),
            description: format!(
                "Shared public audit ledger `{period_slug}` for {}.",
                input.target_wiki_id
            ),
        });
    }

    documents
}

fn build_personal_title<'a>(
    config: &'a WikiStorageConfig,
    username: &str,
    segments: impl IntoIterator<Item = &'a str>,
) -> String {
    build_title(
        &config.personal_namespace,
        username,
        std::iter::once(config.project_root_segment.as_str()).chain(segments),
    )
}

fn build_shared_title<'a>(
    config: &'a WikiStorageConfig,
    owner_username: &str,
    segments: impl IntoIterator<Item = &'a str>,
) -> String {
    build_title(
        &config.shared_namespace,
        owner_username,
        std::iter::once(config.project_root_segment.as_str()).chain(segments),
    )
}

fn build_title<'a>(
    namespace: &str,
    username: &str,
    segments: impl IntoIterator<Item = &'a str>,
) -> String {
    let normalized_username = normalize_title_segment(username);
    let normalized_segments = segments
        .into_iter()
        .map(normalize_title_segment)
        .collect::<Vec<_>>();

    format!(
        "{namespace}:{normalized_username}/{}",
        normalized_segments.join("/")
    )
}

fn normalize_title_segment(raw: &str) -> String {
    let mut segment = String::new();
    let mut last_was_separator = false;

    for character in raw.trim().chars() {
        let mapped = match character {
            '/' | '#' | '<' | '>' | '[' | ']' | '{' | '}' | '|' => '-',
            c if c.is_whitespace() => '_',
            c => c,
        };

        if (mapped == '_' || mapped == '-') && last_was_separator {
            continue;
        }

        last_was_separator = mapped == '_' || mapped == '-';
        segment.push(mapped);
    }

    segment.trim_matches(['_', '-']).to_string()
}

fn default_if_empty(values: &[String], default: &str) -> Vec<String> {
    if values.is_empty() {
        vec![default.to_string()]
    } else {
        values.to_vec()
    }
}

fn document_kind_label(kind: &WikiStorageDocumentKind) -> &'static str {
    match kind {
        WikiStorageDocumentKind::PersonalIndex => "personal-index",
        WikiStorageDocumentKind::PersonalProfile => "personal-profile",
        WikiStorageDocumentKind::PersonalPreferences => "personal-preferences",
        WikiStorageDocumentKind::PersonalQueue { .. } => "personal-queue",
        WikiStorageDocumentKind::PersonalWorkspace { .. } => "personal-workspace",
        WikiStorageDocumentKind::PersonalLabels { .. } => "personal-labels",
        WikiStorageDocumentKind::SharedRegistry { .. } => "shared-registry",
        WikiStorageDocumentKind::SharedTeam { .. } => "shared-team",
        WikiStorageDocumentKind::SharedRuleSet { .. } => "shared-rule-set",
        WikiStorageDocumentKind::SharedTrainingDataset { .. } => "shared-training-dataset",
        WikiStorageDocumentKind::SharedAuditPeriod { .. } => "shared-audit-period",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use futures::executor::block_on;
    use serde_json::json;

    use super::{
        PAYLOAD_BEGIN_MARKER, PAYLOAD_END_MARKER, WikiStorageConfig, WikiStoragePlanInput,
        WikiStorageWriteRequest, build_wiki_storage_plan, load_wiki_storage_document,
        parse_wiki_storage_payload_envelope, render_wiki_storage_document_page,
        render_wiki_storage_index_page, save_wiki_storage_document,
    };
    use crate::config_parser::parse_wiki_config;
    use crate::traits::StubHttpClient;
    use crate::{FlagState, HttpResponse, WikiStorageError};

    fn sample_input() -> WikiStoragePlanInput {
        WikiStoragePlanInput {
            username: "Schiste".to_string(),
            home_wiki_id: "frwiki".to_string(),
            target_wiki_id: "frwiki".to_string(),
            shared_owner_username: "Schiste".to_string(),
            team_slugs: vec!["moderators".to_string()],
            rule_set_slugs: vec!["default".to_string()],
            training_dataset_slugs: vec!["main".to_string()],
            audit_period_slugs: vec!["2026-03".to_string()],
        }
    }

    #[test]
    fn builds_expected_personal_and_shared_titles() {
        let plan = build_wiki_storage_plan(&WikiStorageConfig::default(), &sample_input());

        assert_eq!(plan.personal_root.title, "User:Schiste/SP42");
        assert!(
            plan.personal_documents
                .iter()
                .any(|document| document.title == "User:Schiste/SP42/Queues/frwiki")
        );
        assert_eq!(plan.shared_root.title, "User:Schiste/SP42/frwiki/Registry");
        assert!(
            plan.shared_documents
                .iter()
                .any(|document| document.title == "User:Schiste/SP42/frwiki/Teams/moderators")
        );
    }

    #[test]
    fn index_page_renders_human_readable_links() {
        let plan = build_wiki_storage_plan(&WikiStorageConfig::default(), &sample_input());
        let body = render_wiki_storage_index_page(
            &plan.personal_root,
            &plan.personal_documents,
            &plan.notes,
        );

        assert!(body.contains("== Linked Pages =="));
        assert!(body.contains("[[User:Schiste/SP42/Profile]]"));
        assert!(body.contains("Personal durable state lives on frwiki"));
    }

    #[test]
    fn document_page_embeds_machine_payload_block() {
        let plan = build_wiki_storage_plan(&WikiStorageConfig::default(), &sample_input());
        let document = plan
            .shared_documents
            .iter()
            .find(|document| document.title.ends_with("/Teams/moderators"))
            .expect("team document should exist");
        let page = render_wiki_storage_document_page(
            document,
            &["Public shared team definition.".to_string()],
            &json!({
                "name": "Moderators",
                "members": ["Schiste"]
            }),
        )
        .expect("document page should render");

        assert!(page.contains(PAYLOAD_BEGIN_MARKER));
        assert!(page.contains(PAYLOAD_END_MARKER));
        assert!(page.contains("\"kind\": \"shared-team\""));
        assert!(page.contains("\"members\": ["));
    }

    #[test]
    fn normalizes_problematic_title_segments() {
        let config = WikiStorageConfig::default();
        let input = WikiStoragePlanInput {
            username: "Schiste/Test".to_string(),
            home_wiki_id: "frwiki".to_string(),
            target_wiki_id: "fr wiki".to_string(),
            shared_owner_username: "Schiste/Test".to_string(),
            team_slugs: vec!["core team".to_string()],
            rule_set_slugs: Vec::new(),
            training_dataset_slugs: Vec::new(),
            audit_period_slugs: Vec::new(),
        };
        let plan = build_wiki_storage_plan(&config, &input);

        assert_eq!(plan.personal_root.title, "User:Schiste-Test/SP42");
        assert!(
            plan.personal_documents
                .iter()
                .any(|document| document.title == "User:Schiste-Test/SP42/Queues/fr_wiki")
        );
        assert!(
            plan.shared_documents
                .iter()
                .any(|document| document.title == "User:Schiste-Test/SP42/fr_wiki/Teams/core_team")
        );
    }

    #[test]
    fn parses_embedded_payload_envelope() {
        let plan = build_wiki_storage_plan(&WikiStorageConfig::default(), &sample_input());
        let page = render_wiki_storage_document_page(
            &plan.personal_documents[0],
            &["Compact theme".to_string()],
            &json!({ "theme": "compact" }),
        )
        .expect("document page should render");

        let envelope = parse_wiki_storage_payload_envelope(&page).expect("payload should parse");
        assert_eq!(envelope.kind, "personal-profile");
        assert_eq!(
            envelope
                .data
                .get("theme")
                .and_then(serde_json::Value::as_str),
            Some("compact")
        );
    }

    #[test]
    fn load_document_reads_revision_and_payload() {
        let config = parse_wiki_config(include_str!("../../../configs/frwiki.yaml"))
            .expect("fixture should parse");
        let client = StubHttpClient::new([Ok(HttpResponse {
            status: 200,
            headers: BTreeMap::default(),
            body: br#"{"query":{"pages":[{"title":"User:Schiste/SP42/Profile","pageid":42,"revisions":[{"revid":314,"slots":{"main":{"content":"= Profile =\n<!-- SP42:BEGIN -->\n<syntaxhighlight lang=\"json\">\n{\"project\":\"SP42\",\"version\":1,\"title\":\"User:Schiste/SP42/Profile\",\"kind\":\"personal-profile\",\"site_wiki_id\":\"frwiki\",\"realm\":\"PersonalUserSpace\",\"data\":{\"theme\":\"compact\"}}\n</syntaxhighlight>\n<!-- SP42:END -->"}}}]}]}}"#.to_vec(),
        })]);

        let loaded = block_on(load_wiki_storage_document(
            &client,
            &config,
            "User:Schiste/SP42/Profile",
        ))
        .expect("document should load");

        assert!(loaded.exists);
        assert_eq!(loaded.page_id, Some(42));
        assert_eq!(loaded.revision_id, Some(314));
        assert_eq!(
            loaded
                .envelope
                .expect("payload should exist")
                .data
                .get("theme")
                .and_then(serde_json::Value::as_str),
            Some("compact")
        );
    }

    #[test]
    fn save_document_maps_edit_conflicts() {
        let config = parse_wiki_config(include_str!("../../../configs/frwiki.yaml"))
            .expect("fixture should parse");
        let plan = build_wiki_storage_plan(&WikiStorageConfig::default(), &sample_input());
        let client = StubHttpClient::new([Ok(HttpResponse {
            status: 200,
            headers: BTreeMap::default(),
            body: br#"{"error":{"code":"editconflict","info":"Edit conflict"}}"#.to_vec(),
        })]);

        let error = block_on(save_wiki_storage_document(
            &client,
            &config,
            &WikiStorageWriteRequest {
                document: plan.personal_documents[0].clone(),
                human_summary: vec!["Compact theme".to_string()],
                data: json!({ "theme": "compact" }),
                token: "csrf-token".to_string(),
                baserevid: Some(10),
                tags: vec![],
                watchlist: None,
                create_only: FlagState::Disabled,
                minor: FlagState::Disabled,
                summary: Some("Save SP42 profile".to_string()),
            },
        ))
        .expect_err("conflict should be reported");

        assert!(matches!(error, WikiStorageError::Conflict { .. }));
    }
}
