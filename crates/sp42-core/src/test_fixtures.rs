use crate::WikiConfig;

pub(crate) fn fixture_wiki_config() -> WikiConfig {
    let compiled =
        crate::scoring_policy::load_embedded_compiled_scoring_policy("active/frwiki-vandalism")
            .expect("embedded frwiki scoring policy should compile");
    let mut config =
        serde_yaml::from_str::<WikiConfig>(include_str!("../../../configs/frwiki.yaml"))
            .expect("embedded frwiki config should deserialize");
    config.scoring = compiled.scoring_config;
    config
}
