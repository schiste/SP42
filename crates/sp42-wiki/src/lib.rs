#![forbid(unsafe_code)]

//! Wiki configuration, registry, fixtures, and capability profile contracts.
//!
//! `sp42-wiki` owns the Open/Closed multiwiki boundary: callers ask for a
//! wiki by ID and receive a parsed configuration, while adding a wiki stays a
//! config/registry concern.
//!
//! The `WikiConfig` data shape remains in `sp42-core` until a neutral
//! `sp42-types` crate exists. This crate owns parsing, loading, defaulting, and
//! capability derivation around that shared contract.

pub mod capabilities;
pub mod config;
pub mod errors;
pub mod registry;

pub use capabilities::{
    WikiActionTokenAvailability, WikiCapabilityProfile, WikiCapabilityProfileInput,
    WikiEditingCapabilityProfile, WikiModerationCapabilityProfile, WikiReadCapabilityProfile,
    derive_wiki_capability_profile,
};
pub use config::parse_wiki_config;
pub use errors::{ConfigError, WikiRegistryError};
pub use registry::{
    DEFAULT_WIKI_CONFIG_DIR, SP42_DEFAULT_WIKI_ID, SP42_WIKI_CONFIG_DIR, WikiRegistry,
    load_configs_from_dir,
};
