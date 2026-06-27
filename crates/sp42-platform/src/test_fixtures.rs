use crate::WikiConfig;

/// A representative `frwiki` [`WikiConfig`] with the embedded active scoring
/// policy applied, for use across workspace test suites (gated behind the
/// `test-support` feature).
///
/// # Panics
///
/// Panics if the embedded `frwiki` scoring policy or config fixture fails to
/// compile/deserialize — both are vendored in-repo, so this only fires if those
/// fixtures are corrupted.
#[must_use]
pub fn fixture_wiki_config() -> WikiConfig {
    let compiled =
        crate::scoring_policy::load_embedded_compiled_scoring_policy("active/frwiki-vandalism")
            .expect("embedded frwiki scoring policy should compile");
    let mut config =
        serde_yaml::from_str::<WikiConfig>(include_str!("../../../configs/frwiki.yaml"))
            .expect("embedded frwiki config should deserialize");
    config.scoring = compiled.scoring_config;
    config
}
