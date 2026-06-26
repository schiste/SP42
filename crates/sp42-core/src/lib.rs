#![forbid(unsafe_code)]

//! SP42 domain code (patrolling + references/citation) plus a migration facade
//! over the platform layer.
//!
//! The platform-independent machinery now lives in [`sp42_platform`]; it is
//! re-exported here so existing `sp42_core::*` paths keep resolving while
//! dependents are retargeted to `sp42-platform` directly (ADR-0013). The modules
//! defined locally below are the not-yet-extracted domains: citation
//! verification and the patrol review workflow.
//!
//! ```
//! use sp42_core::{WikiConfig, branding::PROJECT_NAME};
//!
//! let config = WikiConfig {
//!     wiki_id: "frwiki".to_string(),
//!     display_name: "French Wikipedia".to_string(),
//!     api_url: "https://fr.wikipedia.org/w/api.php".parse().unwrap(),
//!     eventstreams_url: "https://stream.wikimedia.org/v2/stream/recentchange".parse().unwrap(),
//!     oauth_authorize_url: "https://meta.wikimedia.org/w/rest.php/oauth2/authorize".parse().unwrap(),
//!     oauth_token_url: "https://meta.wikimedia.org/w/rest.php/oauth2/access_token".parse().unwrap(),
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

// Platform facade: re-export the entire platform surface so existing
// `sp42_core::*` paths (both flat symbols and module paths) keep resolving during
// the migration. This shrinks as dependents retarget to `sp42-platform`.
pub use sp42_platform::*;

// Domain layer (not yet extracted): references/citation + patrol review workflow.
pub mod bare_url_repair;
pub mod citation;
pub mod context_builder;
pub mod review_workbench;
pub mod scoring_evaluation;

pub use bare_url_repair::{
    BareUrlApplyRequest, BareUrlApplyResponse, BareUrlDeclineReason, BareUrlDeclined,
    BareUrlOutcome, BareUrlProposal, BareUrlProposalsRequest, BareUrlProposalsResponse,
    BareUrlReference, bare_url_references, citoid_language, classify_bare_url,
    iso_date_from_epoch_ms, render_bare_url_citation,
};
pub use citation::body_classifier::{BodyUsability, BodyUsabilityReason, classify_body_usability};
pub use citation::citoid::{
    CitoidMetadata, build_citoid_header, build_citoid_request, parse_citoid_response,
};
pub use citation::concurrency::map_with_concurrency;
pub use citation::extract::{
    BlockFailure, CitationUseSite, ExtractOutcome, SkippedReason, SkippedRef, extract_use_sites,
};
pub use citation::locate_quote::{FuzzyLocate, locate_quote, locate_quote_fuzzy};
pub use citation::page::{
    PageVerificationReport, PageVerificationRequest, PageVerificationStats, verify_page,
};
pub use citation::parsing::{
    ParsedVerdict, canonicalize_verdict, parse_repair_response, parse_verdict_response,
};
pub use citation::prompts::ClaimContext;
pub use citation::prompts::{build_repair_prompt, build_verify_prompt};
pub use citation::segment::{Sentence, segment_sentences};
pub use citation::source_fetch::{html_to_text, looks_like_html, recover_wayback_body};
pub use citation::storage::{
    SnapshotEnvelope, VerdictEnvelope, build_snapshot, build_verdict_envelope, load_snapshot,
    load_verdict, store_snapshot, store_verdict,
};
pub use citation::urls::{
    PageTarget, ResolvedUrl, build_article_html_url, check_fetchable_source_url, is_archive_url,
    is_valid_wiki_code, parse_page_target, parse_revision_from_etag, resolve_citation_url,
    rewrite_wayback_url,
};
pub use citation::verdict::{CitationFindingKind, CitationVerdict, SupportLevel, Verdict};
pub use citation::verify::{
    CitationFinding, CitationVerificationRequest, FetchedSource, GroundingAssertion,
    GroundingStatus, LocatedPassage, ModelVerdict, ModelVote, SourceProvenance,
    SourceUnavailableReason, VerificationOutcome, VerifyModelInputs, VerifyOptions,
    assemble_citation_finding, build_model_votes, execute_citation_verify, is_groundable_support,
    sha256_hex, verify_citation_use_site,
};
pub use citation::voting::{BinaryVote, NClassVote, PanelAgreement, binary_vote, n_class_vote};
pub use context_builder::{ContextInputs, build_scoring_context};
pub use review_workbench::{
    PreparedRequestPreview, ReviewWorkbench, build_review_workbench,
    build_session_action_execution_requests,
};
pub use scoring_evaluation::{
    FairnessFixtureCheck, FairnessFixtureSet, InvariantFixtureRule, InvariantFixtureSet,
    RankingFixtureComparison, RankingFixtureSet, RegressionFixtureCase, RegressionFixtureSet,
    parse_fairness_fixture_set, parse_invariant_fixture_set, parse_ranking_fixture_set,
    parse_regression_fixture_set,
};
