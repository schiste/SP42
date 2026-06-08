//! Shared wiki fixtures for downstream crate tests.

use sp42_core::WikiConfig;

pub const FRWIKI_CONFIG_YAML: &str = include_str!("../../../configs/frwiki.yaml");

/// Return the canonical frwiki fixture parsed through the production wiki parser.
///
/// # Panics
///
/// Panics if the embedded frwiki fixture no longer parses as a valid wiki config.
#[must_use]
pub fn frwiki_config() -> WikiConfig {
    crate::parse_wiki_config(FRWIKI_CONFIG_YAML).expect("embedded frwiki config should parse")
}
