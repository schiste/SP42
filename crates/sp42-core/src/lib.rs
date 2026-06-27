#![forbid(unsafe_code)]

//! SP42 migration facade (ADR-0013). All code has been extracted into the
//! platform layer ([`sp42_platform`]) and the domain crates ([`sp42_citation`],
//! [`sp42_patrol`]); this crate re-exports them so existing `sp42_core::*` paths
//! keep resolving while dependents are retargeted. It owns no code of its own
//! and is retired in the relocation slice.
//!
//! ```
//! use sp42_core::{WikiConfig, branding::PROJECT_NAME};
//!
//! let config = WikiConfig {
//!     wiki_id: "frwiki".to_string(),
//!     display_name: "French Wikipedia".to_string(),
//!     api_url: "https://fr.wikipedia.org/w/api.php".parse().expect("valid url"),
//!     eventstreams_url: "https://stream.wikimedia.org/v2/stream/recentchange".parse().expect("valid url"),
//!     oauth_authorize_url: "https://meta.wikimedia.org/w/rest.php/oauth2/authorize".parse().expect("valid url"),
//!     oauth_token_url: "https://meta.wikimedia.org/w/rest.php/oauth2/access_token".parse().expect("valid url"),
//!     liftwing_url: None,
//!     coordination_url: None,
//!     parsoid_url: None,
//!     inference_url: None,
//!     namespace_allowlist: vec![0],
//!     scoring_policy_ref: "active/frwiki-vandalism".to_string(),
//!     scoring: Default::default(),
//!     templates: Default::default(),
//! };
//!
//! assert_eq!(PROJECT_NAME, "SP42");
//! assert_eq!(config.wiki_id, "frwiki");
//! ```

// Migration facade: re-export the extracted domains (references + patrolling),
// each of which re-exports the whole platform surface (`pub use sp42_platform::*`),
// so every existing `sp42_core::*` path keeps resolving. sp42-core now owns no
// code of its own; it is a pure facade, retired once dependents retarget to
// `sp42-platform` / `sp42-citation` / `sp42-patrol` directly.
pub use sp42_citation::*;
pub use sp42_patrol::*;
