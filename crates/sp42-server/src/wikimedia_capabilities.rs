use std::collections::BTreeSet;
use std::env;
use std::fmt;

use reqwest::Client;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use tracing::{debug, warn};

use sp42_core::{
    DevAuthActionTokenAvailability, DevAuthCapabilityReadiness, DevAuthCapabilityReport,
    DevAuthDerivedCapabilities, DevAuthEditCapabilities, DevAuthModerationCapabilities,
    DevAuthProbeAcceptance, LocalOAuthConfigStatus, WikiConfig, parse_wiki_config,
};

const FRWIKI_CONFIG: &str = include_str!("../../../configs/frwiki.yaml");
const PROFILE_URL: &str = "https://meta.wikimedia.org/w/rest.php/oauth2/resource/profile";
const ERROR_BODY_PREVIEW_LIMIT: usize = 240;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityProbeTargets {
    pub profile_url: String,
    pub api_url: Option<String>,
}

impl Default for CapabilityProbeTargets {
    fn default() -> Self {
        let profile_url = env::var("SP42_TEST_PROFILE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| PROFILE_URL.to_string());
        let api_url = env::var("SP42_TEST_API_URL")
            .ok()
            .filter(|value| !value.trim().is_empty());

        Self {
            profile_url,
            api_url,
        }
    }
}

#[derive(Debug, serde::Serialize, Deserialize)]
struct OAuthProfile {
    username: String,
    #[serde(default)]
    grants: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct UserInfoEnvelope {
    query: UserInfoQuery,
}

#[derive(Debug, Deserialize)]
struct UserInfoQuery {
    userinfo: UserInfoPayload,
}

#[derive(Debug, Deserialize)]
struct UserInfoPayload {
    name: String,
    #[serde(default)]
    groups: Vec<String>,
    #[serde(default)]
    rights: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct TokenEnvelope {
    query: TokenQuery,
}

#[derive(Debug, Deserialize)]
struct TokenQuery {
    tokens: TokenPayload,
}

#[derive(Debug, Deserialize)]
struct TokenPayload {
    csrftoken: Option<String>,
    patroltoken: Option<String>,
    rollbacktoken: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CapabilityProbeStage {
    Profile,
    UserInfo,
    Tokens,
}

impl CapabilityProbeStage {
    const fn label(self) -> &'static str {
        match self {
            Self::Profile => "profile",
            Self::UserInfo => "userinfo",
            Self::Tokens => "tokens",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CapabilityProbeError {
    stage: CapabilityProbeStage,
    endpoint: String,
    params: Option<String>,
    message: String,
    status: Option<u16>,
    body_preview: Option<String>,
}

impl fmt::Display for CapabilityProbeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} probe failed: endpoint={}",
            self.stage.label(),
            self.endpoint
        )?;
        if let Some(params) = &self.params {
            write!(formatter, " params={params}")?;
        }
        if let Some(status) = self.status {
            write!(formatter, " status={status}")?;
        }
        write!(formatter, " message={}", self.message)?;
        if let Some(body_preview) = &self.body_preview {
            write!(formatter, " body={body_preview}")?;
        }
        Ok(())
    }
}

pub async fn probe_with_targets(
    client: &Client,
    token: Option<&str>,
    oauth_status: &LocalOAuthConfigStatus,
    wiki_id: &str,
    targets: &CapabilityProbeTargets,
) -> DevAuthCapabilityReport {
    let mut report = DevAuthCapabilityReport {
        wiki_id: wiki_id.to_string(),
        ..DevAuthCapabilityReport::default()
    };

    let Some(token) = token.filter(|value| !value.trim().is_empty()) else {
        report.error =
            Some("WIKIMEDIA_ACCESS_TOKEN is missing from .env.wikimedia.local".to_string());
        report
            .notes
            .push("No local dev token is configured.".to_string());
        return report;
    };

    let config = match config_for_wiki(wiki_id) {
        Ok(config) => config,
        Err(error) => {
            report.checked = true;
            report.error = Some(error);
            return report;
        }
    };

    match fetch_report(client, token, &config, targets).await {
        Ok(mut fetched) => {
            fetched.notes.extend(base_notes(oauth_status));
            fetched
        }
        Err(error) => {
            report.checked = true;
            report.error = Some(error);
            report.notes.extend(base_notes(oauth_status));
            report
        }
    }
}

async fn fetch_report(
    client: &Client,
    token: &str,
    config: &WikiConfig,
    targets: &CapabilityProbeTargets,
) -> Result<DevAuthCapabilityReport, String> {
    debug!(
        wiki_id = config.wiki_id.as_str(),
        "probing wikimedia capabilities"
    );
    let profile = fetch_profile(client, token, targets).await?;
    let userinfo = fetch_userinfo(client, token, config, targets).await?;
    let tokens = fetch_action_tokens(client, token, config, targets).await?;

    Ok(derive_report(config, profile, userinfo, &tokens))
}

async fn fetch_profile(
    client: &Client,
    token: &str,
    targets: &CapabilityProbeTargets,
) -> Result<OAuthProfile, String> {
    fetch_json(
        client.get(&targets.profile_url).bearer_auth(token),
        CapabilityProbeStage::Profile,
        &targets.profile_url,
        None,
    )
    .await
}

async fn fetch_userinfo(
    client: &Client,
    token: &str,
    config: &WikiConfig,
    targets: &CapabilityProbeTargets,
) -> Result<UserInfoPayload, String> {
    let endpoint = api_url(config, targets);
    let params = [
        ("action", "query"),
        ("meta", "userinfo"),
        ("uiprop", "rights|groups"),
        ("format", "json"),
        ("formatversion", "2"),
    ];
    let envelope: UserInfoEnvelope = fetch_json(
        client
            .get(endpoint.clone())
            .bearer_auth(token)
            .query(&params),
        CapabilityProbeStage::UserInfo,
        &endpoint,
        Some(query_string(&params)),
    )
    .await?;
    Ok(envelope.query.userinfo)
}

async fn fetch_action_tokens(
    client: &Client,
    token: &str,
    config: &WikiConfig,
    targets: &CapabilityProbeTargets,
) -> Result<TokenPayload, String> {
    let endpoint = api_url(config, targets);
    let params = [
        ("action", "query"),
        ("meta", "tokens"),
        ("type", "patrol|rollback|csrf"),
        ("format", "json"),
        ("formatversion", "2"),
    ];
    let envelope: TokenEnvelope = fetch_json(
        client
            .get(endpoint.clone())
            .bearer_auth(token)
            .query(&params),
        CapabilityProbeStage::Tokens,
        &endpoint,
        Some(query_string(&params)),
    )
    .await?;
    Ok(envelope.query.tokens)
}

fn api_url(config: &WikiConfig, targets: &CapabilityProbeTargets) -> String {
    targets
        .api_url
        .clone()
        .unwrap_or_else(|| config.api_url.to_string())
}

fn derive_report(
    config: &WikiConfig,
    profile: OAuthProfile,
    userinfo: UserInfoPayload,
    tokens: &TokenPayload,
) -> DevAuthCapabilityReport {
    let oauth_grants = sort_strings(profile.grants);
    let wiki_groups = sort_strings(userinfo.groups);
    let wiki_rights = sort_strings(userinfo.rights);

    let grant_set: BTreeSet<_> = oauth_grants.iter().map(String::as_str).collect();
    let right_set: BTreeSet<_> = wiki_rights.iter().map(String::as_str).collect();

    let csrf_token_available = token_present(tokens.csrftoken.as_deref());
    let patrol_token_available = token_present(tokens.patroltoken.as_deref());
    let rollback_token_available = token_present(tokens.rollbacktoken.as_deref());

    let can_edit =
        grant_set.contains("editpage") && right_set.contains("edit") && csrf_token_available;
    let can_patrol =
        grant_set.contains("patrol") && right_set.contains("patrol") && patrol_token_available;
    let can_rollback = grant_set.contains("rollback")
        && right_set.contains("rollback")
        && rollback_token_available;

    let mut notes = vec![
        "SP42 recentchanges reads do not require authentication; the token is needed for user-linked actions and rights validation.".to_string(),
        format!(
            "Capability probe verified profile, userinfo, and token endpoints for {}.",
            config.wiki_id
        ),
    ];

    if grant_set.contains("rollback") && !right_set.contains("rollback") {
        notes.push(format!(
            "The token carries the OAuth rollback grant, but the account does not currently have the rollback right on {}.",
            config.wiki_id
        ));
    }

    if rollback_token_available && !right_set.contains("rollback") {
        notes.push(
            "A rollback token was returned by the API, but SP42 still treats rollback as unavailable because the wiki right is missing.".to_string(),
        );
    }

    if grant_set.contains("patrol") && !right_set.contains("patrol") {
        notes.push(format!(
            "The token carries the OAuth patrol grant, but the account does not currently have the patrol right on {}.",
            config.wiki_id
        ));
    }

    DevAuthCapabilityReport {
        checked: true,
        wiki_id: config.wiki_id.clone(),
        username: Some(profile.username).or(Some(userinfo.name)),
        oauth_grants,
        wiki_groups,
        wiki_rights,
        acceptance: DevAuthProbeAcceptance {
            profile_accepted: true,
            userinfo_accepted: true,
        },
        token_availability: DevAuthActionTokenAvailability {
            csrf_token_available,
            patrol_token_available,
            rollback_token_available,
        },
        capabilities: DevAuthDerivedCapabilities {
            read: DevAuthCapabilityReadiness {
                can_authenticate: true,
                can_query_userinfo: true,
                can_read_recent_changes: true,
            },
            editing: DevAuthEditCapabilities {
                can_edit,
                can_undo: can_edit,
            },
            moderation: DevAuthModerationCapabilities {
                can_patrol,
                can_rollback,
            },
        },
        notes,
        error: None,
    }
}

async fn fetch_json<T>(
    request: reqwest::RequestBuilder,
    stage: CapabilityProbeStage,
    endpoint: &str,
    params: Option<String>,
) -> Result<T, String>
where
    T: DeserializeOwned,
{
    let response = request.send().await.map_err(|error| {
        CapabilityProbeError {
            stage,
            endpoint: endpoint.to_string(),
            params: params.clone(),
            message: error.to_string(),
            status: None,
            body_preview: None,
        }
        .to_string()
    })?;

    let status = response.status();
    let body = response.text().await.map_err(|error| {
        CapabilityProbeError {
            stage,
            endpoint: endpoint.to_string(),
            params: params.clone(),
            message: format!("failed to read response body: {error}"),
            status: Some(status.as_u16()),
            body_preview: None,
        }
        .to_string()
    })?;

    if !status.is_success() {
        let error = CapabilityProbeError {
            stage,
            endpoint: endpoint.to_string(),
            params,
            message: "upstream returned a non-success status".to_string(),
            status: Some(status.as_u16()),
            body_preview: Some(preview_body(&body)),
        };
        warn!(error = %error, "wikimedia capability probe request failed");
        return Err(error.to_string());
    }

    serde_json::from_str(&body).map_err(|error| {
        let error = CapabilityProbeError {
            stage,
            endpoint: endpoint.to_string(),
            params,
            message: format!("response payload was invalid JSON: {error}"),
            status: Some(status.as_u16()),
            body_preview: Some(preview_body(&body)),
        };
        warn!(error = %error, "wikimedia capability probe response failed to decode");
        error.to_string()
    })
}

fn query_string(params: &[(&str, &str)]) -> String {
    params
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("&")
}

fn preview_body(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return "empty".to_string();
    }

    let preview = trimmed
        .chars()
        .take(ERROR_BODY_PREVIEW_LIMIT)
        .collect::<String>();
    if trimmed.chars().count() > ERROR_BODY_PREVIEW_LIMIT {
        format!("{preview}...")
    } else {
        preview
    }
}

pub(crate) fn config_for_wiki(wiki_id: &str) -> Result<WikiConfig, String> {
    let config = parse_wiki_config(FRWIKI_CONFIG)
        .map_err(|error| format!("wiki config was invalid: {error}"))?;

    if config.wiki_id == wiki_id {
        Ok(config)
    } else {
        Err(format!("unsupported local capability wiki_id: {wiki_id}"))
    }
}

fn base_notes(oauth_status: &LocalOAuthConfigStatus) -> Vec<String> {
    let mut notes = Vec::new();

    if !oauth_status.client_id_present {
        notes.push(
            "WIKIMEDIA_CLIENT_APPLICATION_KEY is not set in .env.wikimedia.local.".to_string(),
        );
    }
    if !oauth_status.client_secret_present {
        notes.push(
            "WIKIMEDIA_CLIENT_APPLICATION_SECRET is not set in .env.wikimedia.local.".to_string(),
        );
    }

    notes
}

fn sort_strings(values: Vec<String>) -> Vec<String> {
    let mut values = values;
    values.sort();
    values
}

fn token_present(value: Option<&str>) -> bool {
    value.is_some_and(|entry| !entry.is_empty() && entry != "+\\")
}

#[cfg(test)]
mod tests {
    use reqwest::Client;
    use serde::Deserialize;
    use sp42_core::LocalOAuthConfigStatus;
    use sp42_core::parse_wiki_config;

    use super::{
        CapabilityProbeTargets, OAuthProfile, TokenPayload, UserInfoPayload, derive_report,
        probe_with_targets,
    };

    #[test]
    fn derives_rollback_as_unavailable_without_wiki_right() {
        let config = parse_wiki_config(include_str!("../../../configs/frwiki.yaml"))
            .expect("config should parse");
        let report = derive_report(
            &config,
            OAuthProfile {
                username: "Schiste".to_string(),
                grants: vec![
                    "basic".to_string(),
                    "editpage".to_string(),
                    "patrol".to_string(),
                    "rollback".to_string(),
                ],
            },
            UserInfoPayload {
                name: "Schiste".to_string(),
                groups: vec![
                    "*".to_string(),
                    "user".to_string(),
                    "autoconfirmed".to_string(),
                    "autopatrolled".to_string(),
                ],
                rights: vec!["edit".to_string(), "patrol".to_string()],
            },
            &TokenPayload {
                csrftoken: Some("csrf".to_string()),
                patroltoken: Some("patrol".to_string()),
                rollbacktoken: Some("rollback".to_string()),
            },
        );

        assert!(report.capabilities.editing.can_edit);
        assert!(report.capabilities.moderation.can_patrol);
        assert!(!report.capabilities.moderation.can_rollback);
        assert!(
            report
                .notes
                .iter()
                .any(|note| note.contains("rollback right"))
        );
    }

    #[tokio::test]
    async fn probe_with_targets_uses_custom_profile_endpoint() {
        use axum::extract::Query;
        use axum::routing::get;
        use axum::{Json, Router};
        use tokio::net::TcpListener;

        #[derive(Debug, Deserialize)]
        struct ApiQuery {
            meta: Option<String>,
            #[serde(rename = "type")]
            r#type: Option<String>,
        }

        async fn profile() -> Json<OAuthProfile> {
            Json(OAuthProfile {
                username: "Schiste".to_string(),
                grants: vec![
                    "basic".to_string(),
                    "editpage".to_string(),
                    "patrol".to_string(),
                    "rollback".to_string(),
                ],
            })
        }

        async fn api(Query(params): Query<ApiQuery>) -> Json<serde_json::Value> {
            let response = match (params.meta.as_deref(), params.r#type.as_deref()) {
                (Some("userinfo"), None) => serde_json::json!({
                    "query": {
                        "userinfo": {
                            "name": "Schiste",
                            "groups": ["*", "user", "autoconfirmed", "autopatrolled"],
                            "rights": ["edit", "patrol", "rollback"]
                        }
                    }
                }),
                (Some("tokens"), Some("patrol|rollback|csrf")) => {
                    serde_json::json!({
                        "query": {
                            "tokens": {
                                "csrftoken": "csrf",
                                "patroltoken": "patrol",
                                "rollbacktoken": "rollback"
                            }
                        }
                    })
                }
                _ => serde_json::json!({ "error": "unexpected request" }),
            };

            Json(response)
        }

        let router = Router::new()
            .route("/oauth2/resource/profile", get(profile))
            .route("/w/api.php", get(api));

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener should bind");
        let addr = listener.local_addr().expect("listener address");
        let server = tokio::spawn(async move {
            axum::serve(listener, router)
                .await
                .expect("mock capability server should run");
        });

        let report = probe_with_targets(
            &Client::new(),
            Some("token"),
            &LocalOAuthConfigStatus {
                client_id_present: true,
                client_secret_present: true,
                access_token_present: true,
            },
            "frwiki",
            &CapabilityProbeTargets {
                profile_url: format!("http://{addr}/oauth2/resource/profile"),
                api_url: Some(format!("http://{addr}/w/api.php")),
            },
        )
        .await;

        server.abort();

        assert!(report.checked);
        assert!(report.capabilities.read.can_authenticate);
        assert!(report.capabilities.read.can_query_userinfo);
        assert!(report.capabilities.editing.can_edit);
        assert!(report.capabilities.moderation.can_patrol);
        assert!(report.capabilities.moderation.can_rollback);
        assert!(
            report
                .notes
                .iter()
                .any(|note| note.contains("Capability probe verified"))
        );
        assert_eq!(report.wiki_id, "frwiki");
        assert_eq!(report.username.as_deref(), Some("Schiste"));
    }
}
