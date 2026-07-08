#![forbid(unsafe_code)]

//! SP42 **references** domain: citation verification (PRD-0001, ADR-0006/0007/
//! 0008/0009) and bare-URL repair.
//!
//! This crate consumes the platform layer ([`sp42_platform`]) and owns no
//! platform mechanisms of its own. Reusable citation primitives (voting,
//! bounded concurrency, sentence segmentation, fuzzy quote location) are kept
//! together here for now; promotion of the genuinely domain-agnostic ones to
//! the platform is tracked in #69–#73.
//!
//! The platform surface is re-exported so the citation modules' existing
//! `crate::wikitext_editor` / `crate::errors` / `crate::types` paths resolve
//! unchanged after the move out of `sp42-core` (ADR-0013).

pub use sp42_platform::*;
pub use sp42_reporting::*;

pub mod bare_url_repair;
pub mod citation;
pub mod citation_finding;
pub mod citation_page_report;

pub use bare_url_repair::{
    BareUrlApplyRequest, BareUrlApplyResponse, BareUrlDeclineReason, BareUrlDeclined,
    BareUrlOutcome, BareUrlProposal, BareUrlProposalsRequest, BareUrlProposalsResponse,
    BareUrlReference, bare_url_references, citoid_language, classify_bare_url,
    iso_date_from_epoch_ms, render_bare_url_citation,
};
pub use citation::body_classifier::{
    BodyUsability, BodyUsabilityReason, classify_body_usability, classify_source_usability,
};
pub use citation::citoid::{
    CitoidMetadata, build_citoid_header, build_citoid_request, parse_citoid_response,
};
pub use citation::concurrency::map_with_concurrency;
pub use citation::enrich::{
    EnrichmentCandidate, EnrichmentProposal, OpenLibraryRecord, enrichment_candidates,
    isbn10_from_isbn13, isbn13_from_isbn10, parse_record, propose_from_candidate,
};
pub use citation::extract::{
    BlockFailure, BookUseSite, CitationUseSite, ExtractOutcome, SkippedReason, SkippedRef,
    extract_use_sites,
};
pub use citation::locate_quote::{FuzzyLocate, locate_quote, locate_quote_fuzzy};
pub use citation::openlibrary::{
    BookResolution, BookResolutionOutcome, OPEN_LIBRARY_BOOKS_API, OPEN_LIBRARY_READ_API_BASE,
    OpenLibraryEdition, ScanAvailability, ScanItem, bibkey, build_catalog_lookup_request,
    build_scan_availability_request, parse_catalog_lookup, parse_scan_availability,
    resolve_book_source,
};
pub use citation::openlibrary_apply::{
    ApplyLane, ApplyOutcome, ENRICHMENT_WRITE_LANE_ENABLED, EditForm, OPEN_LIBRARY_ORIGIN,
    OpenLibrarySession, apply_proposal_to_record, build_edit_form_request,
    build_form_submit_request, build_login_request, build_record_request, build_rest_apply_request,
    execute_enrichment_apply, fill_edit_form, form_field_name, form_submit_succeeded,
    parse_edit_form, parse_login_response, verify_applied,
};
pub use citation::page::{
    PageVerificationReport, PageVerificationRequest, PageVerificationStats, verify_page,
};
pub use citation::parsing::{
    ParsedVerdict, canonicalize_verdict, parse_repair_response, parse_verdict_response,
};
pub use citation::prompts::ClaimContext;
pub use citation::prompts::{build_repair_prompt, build_verify_prompt};
pub use citation::search_inside::{
    ARCHIVE_METADATA_BASE, BookGroundingPreparation, BookSnippetBody, ItemLocation,
    SearchInsideMatch, SearchInsideResult, build_item_metadata_request,
    build_search_inside_request, extract_ocaid, parse_item_metadata, parse_search_inside,
    prepare_book_grounding, scan_deep_link, search_query,
};
pub use citation::segment::{Sentence, segment_sentences};
pub use citation::source_fetch::{html_to_text, looks_like_html, recover_wayback_body};
pub use citation::storage::{
    SnapshotEnvelope, VerdictEnvelope, build_snapshot, build_verdict_envelope, load_snapshot,
    load_verdict, store_snapshot, store_verdict,
};
pub use citation::urls::{
    PageTarget, ResolvedUrl, build_article_html_url, is_archive_url, is_valid_wiki_code,
    parse_page_target, parse_revision_from_etag, resolve_citation_url, rewrite_wayback_url,
};
pub use citation::verdict::{CitationFindingKind, CitationVerdict, SupportLevel, Verdict};
pub use citation::verify::{
    BookScanProvenance, CitationFinding, CitationVerificationRequest, FetchedSource,
    GroundingAssertion, GroundingStatus, LocatedPassage, ModelVerdict, ModelVote, SourceProvenance,
    SourceUnavailableReason, VerificationOutcome, VerifyModelInputs, VerifyOptions,
    assemble_citation_finding, book_scan_unavailable_outcome, book_searched_not_supported_outcome,
    build_model_votes, execute_citation_verify, fetch_source, is_groundable_support, sha256_hex,
    verify_citation_use_site,
};
pub use citation::voting::{BinaryVote, NClassVote, PanelAgreement, binary_vote, n_class_vote};
pub use citation_finding::{
    FindingGroup, GroundingCaveat, body_usability_label, finding_is_problem, finding_severity_rank,
    grounding_caveat, is_support, panel_agreement_label, source_unavailable_detail,
};
pub use citation_page_report::{
    page_verification_report_to_document, render_page_verification_markdown,
    render_page_verification_text,
};
