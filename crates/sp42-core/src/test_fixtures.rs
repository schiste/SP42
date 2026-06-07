use crate::{WikiConfig, WikiTemplates};

pub(crate) fn fixture_wiki_config() -> WikiConfig {
    let compiled =
        crate::scoring_policy::load_embedded_compiled_scoring_policy("active/frwiki-vandalism")
            .expect("embedded frwiki scoring policy should compile");

    WikiConfig {
        wiki_id: "frwiki".to_string(),
        display_name: "French Wikipedia".to_string(),
        api_url: "https://fr.wikipedia.org/w/api.php"
            .parse()
            .expect("fixture api_url should parse"),
        eventstreams_url: "https://stream.wikimedia.org/v2/stream/recentchange"
            .parse()
            .expect("fixture eventstreams_url should parse"),
        oauth_authorize_url: "https://meta.wikimedia.org/w/rest.php/oauth2/authorize"
            .parse()
            .expect("fixture oauth_authorize_url should parse"),
        oauth_token_url: "https://meta.wikimedia.org/w/rest.php/oauth2/access_token"
            .parse()
            .expect("fixture oauth_token_url should parse"),
        liftwing_url: Some(
            "https://api.wikimedia.org/service/lw/inference/v1/models/revertrisk-language-agnostic:predict"
                .parse()
                .expect("fixture liftwing_url should parse"),
        ),
        coordination_url: None,
        namespace_allowlist: vec![0, 2, 4, 6, 10, 14],
        scoring_policy_ref: "active/frwiki-vandalism".to_string(),
        scoring: compiled.scoring_config.clone(),
        templates: WikiTemplates::default(),
    }
}
