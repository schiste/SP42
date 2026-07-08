//! The citation-verification contract + edge + grounding gate (ADR-0008, ADR-0007 §5).
//!
//! Layering:
//! - **Contract types** ([`CitationVerificationRequest`], [`CitationFinding`], …) — the
//!   read-only Finding surface (ADR-0008 §1/§2). No numeric confidence field; a
//!   `CitationFinding` derives `Eq`.
//! - **Per-model edge** ([`execute_citation_verify`]) — one model, one verdict, over the
//!   provider-agnostic [`ModelClient`] boundary (ADR-0006 Decision 7); the response
//!   parser ends in a validate gate that defaults an unrecoverable response to *not
//!   supported*, never a support judgment. Each call yields a [`ModelInvocation`]
//!   fingerprint for audit/replay (ADR-0006 Decision 8).
//! - **Grounding gate** ([`assemble_citation_finding`]) — pure: votes the panel
//!   (ADR-0006), then independently re-locates the winning quote in the fetched bytes. The
//!   surfaced verdict is the panel's *judgment*; the gate sets `grounding_status`
//!   (`Located`/`Unlocated`) — an unverified `Supported`/`Partial` is surfaced honestly but
//!   is never **groundable** ([`is_groundable_support`], the only autonomous-action gate),
//!   so the model is never trusted on its word (SP42#25 layer 6; refines ADR-0007 §5).
//! - **Orchestration** ([`verify_citation_use_site`]) — async: fetch the source once over
//!   the injected `HttpClient`, run the deterministic body-usability gate (short-circuit
//!   to `SourceUnavailable` with no model call), then fan the panel out over the
//!   `ModelClient` with bounded concurrency and assemble a [`VerificationOutcome`].
//!
//! The per-model edge needs the **fetched source body**, which
//! [`CitationVerificationRequest`] (claim + URL) does not carry; it therefore takes a
//! prepared [`VerifyModelInputs`].

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sp42_types::{ModelClient, ModelCompletionRequest, ModelInvocation, ModelRef, SamplingParams};
use url::Url;

use super::body_classifier::{BodyUsabilityReason, classify_source_usability};
use super::citoid::{
    CitoidMetadata, build_citoid_header, build_citoid_request, parse_citoid_response,
};
use super::concurrency::map_with_concurrency;
use super::locate_quote::{locate_quote, locate_quote_fuzzy};
use super::parsing::{ParsedVerdict, parse_repair_response, parse_verdict_response};
use super::prompts::{ClaimContext, build_repair_prompt, build_verify_prompt};
use super::source_fetch::{html_to_text, looks_like_html, recover_wayback_body};
use super::urls::rewrite_wayback_url;
use super::verdict::{CitationFindingKind, CitationVerdict, SupportLevel, Verdict};
use super::voting::{PanelAgreement, n_class_vote};
use crate::errors::CitationVerificationError;
use crate::traits::{Clock, HttpClient};
use crate::types::{HttpMethod, HttpRequest};

/// The schema version stamped on a [`CitationFinding`] (ADR-0008 §6).
pub const SCHEMA_VERSION: u32 = 1;

/// The operator-facing verification request: a claim, its source URL, and revision
/// context (ADR-0008 §1). Carries no token and no editor identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CitationVerificationRequest {
    /// The wiki id (keyed as the review surface keys diff loading).
    pub wiki_id: String,
    /// The revision id.
    pub rev_id: u64,
    /// The article title.
    pub title: String,
    /// The claim text to verify.
    pub claim: String,
    /// The cited source URL.
    pub source_url: Url,
}

/// A verbatim passage located in the fetched source, with its byte offset (ADR-0008 §2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocatedPassage {
    /// The verbatim quote located in the source.
    pub quote: String,
    /// Byte offset of the match in the fetched source.
    pub offset: usize,
}

/// Provenance of the really-fetched source (ADR-0008 §2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceProvenance {
    /// The source URL that was fetched.
    pub url: Url,
    /// SHA-256 hex of the extracted source body (the grounded bytes).
    pub content_hash: String,
    /// Fetch time in epoch ms, from the injected `Clock`.
    pub fetched_at: i64,
    /// The HTTP status the fetch returned, when known. Distinguishes a failed fetch
    /// (e.g. `403`/`404` permanent, `429`/`503` retryable) from a `200` whose body was
    /// merely unusable — both surface as `SourceUnavailable`, so the status is the only
    /// signal that tells a retry path which is which. `None` for records that pre-date
    /// this field or were replayed from a snapshot that did not capture it.
    #[serde(default)]
    pub http_status: Option<u16>,
}

/// The machine-checkable grounding assertion the gate re-verifies (ADR-0008 §2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GroundingAssertion {
    /// Grounds a support verdict on a passage string-located in the fetched bytes.
    LocatedQuote {
        /// The verbatim quote.
        quote: String,
        /// SHA-256 hex of the fetched body the quote was located in.
        source_hash: String,
        /// Byte offset of the match.
        offset: usize,
    },
    /// Grounds a no-quote verdict on "the source was actually fetched this session".
    SourceFetched {
        /// SHA-256 hex of the fetched body.
        source_hash: String,
    },
}

/// Whether a surfaced support verdict was CONFIRMED in the fetched source — the grounding
/// axis, orthogonal to the verdict (SP42#25 layer 6). The verdict is the panel's *judgment*;
/// this records whether its supporting quote string-located. Consumed by a human reviewer
/// (CLI / report) and the audit record; an autonomous action path must require [`Located`]
/// via [`is_groundable_support`] (SP42 never auto-edits, so this is honest triage, not a
/// silent verdict rewrite — refines the ADR-0007 §5 anti-fabrication gate).
///
/// [`Located`]: GroundingStatus::Located
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum GroundingStatus {
    /// The support quote located verbatim in the fetched source.
    Located,
    /// The quote did not locate verbatim, but the guarded fuzzy match (SP42#25 layer 5)
    /// found the backing passage in the fetched source — the surfaced passage is the
    /// SOURCE's own text. Weighable by a human; never sufficient for an autonomous path
    /// ([`is_groundable_support`] requires [`Located`]).
    ///
    /// [`Located`]: GroundingStatus::Located
    LocatedFuzzy,
    /// The panel judged `Supported`/`Partial` but the quote did not locate — surfaced
    /// honestly as *unverified*; a human may weigh it, an autonomous path never may.
    Unlocated,
    /// No supporting quote is expected (`NotSupported` / `SourceUnavailable`).
    #[default]
    NotApplicable,
}

/// Book-scan provenance for a finding verified against an Internet Archive
/// search-inside snippet (PRD-0009 Layer 2): which scan the snippet came
/// from, the scanned page the passage was found on (which can differ from the
/// cited page — reprints, front-matter offsets), and the cited page echoed so
/// the mismatch surfaces to the operator instead of a false `not_supported`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookScanProvenance {
    /// The archive.org item id of the scan.
    pub ocaid: String,
    /// The scanned page the located passage was found on, when attributable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scanned_page: Option<u32>,
    /// The cite template's `page=` value, echoed for the mismatch signal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cited_page: Option<String>,
    /// Refined grounding note for the operator ("no full-text index",
    /// "item metadata unreachable", …) when the scan could not be searched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Why a `SourceUnavailable` verdict was reached. Derived from the fetch
/// status; lets a reviewer tell a dead link from a source we fetched but
/// could not read (PDF / JS shell / wrong page).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceUnavailableReason {
    /// The source could not be fetched (missing / non-2xx status) — link rot.
    Unreachable,
    /// The source was fetched (2xx) but the panel could not use its content
    /// (e.g. PDF, JavaScript viewer shell, or a wrong/redirected page).
    Unusable,
}

impl SourceUnavailableReason {
    /// Stable lowercase label (the single source of truth for the serde
    /// `snake_case` rename and any human-facing rendering).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unreachable => "unreachable",
            Self::Unusable => "unusable",
        }
    }
}

/// The read-only verification result — a Finding, never an action (ADR-0008 §2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CitationFinding {
    /// The finding kind (single value today).
    pub kind: CitationFindingKind,
    /// The panel's voted categorical *judgment* (ADR-0007). NOT rewritten by the grounding
    /// gate — an unverified support stays `Supported`/`Partial` with `grounding_status`
    /// `Unlocated` (SP42#25 layer 6).
    pub verdict: CitationVerdict,
    /// Whether the support verdict was confirmed in the source (the grounding axis).
    #[serde(default)]
    pub grounding_status: GroundingStatus,
    /// For a `SourceUnavailable` verdict, why the source was unavailable;
    /// `None` for any other verdict.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_unavailable_reason: Option<SourceUnavailableReason>,
    /// When the verdict is `SourceUnavailable` because the body was fetched but
    /// unusable, the specific classifier reason (PDF, viewer shell, paywall, …).
    /// `None` for usable sources and for unreachable (non-2xx) sources.
    #[serde(default)]
    pub unusable_reason: Option<BodyUsabilityReason>,
    /// Measured agreement among the panel's votes (ADR-0006).
    pub agreement: PanelAgreement,
    /// The winning verdict's located passage, or `None`.
    #[serde(default)]
    pub passage: Option<LocatedPassage>,
    /// Provenance of the really-fetched source.
    pub provenance: SourceProvenance,
    /// A bounded, reviewer-facing excerpt of the extracted source text — a window
    /// around the located quote, or the head of the body — so the report can show
    /// *what the panel read* without carrying the whole source. Display only,
    /// never grounded; `None` for an unreadable / dead source. Back-compatible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_excerpt: Option<String>,
    /// Best-effort Citoid bibliographic metadata for the source (title, author,
    /// publication, date) when available — especially useful to identify a source
    /// the tool could not read (PDF / paywall). Context only; never grounded.
    /// Back-compatible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<CitoidMetadata>,
    /// The machine-checkable grounding assertion.
    pub grounding: GroundingAssertion,
    /// Document-order position of this use-site (ADR-0007 §2).
    #[serde(default)]
    pub use_site_ordinal: u32,
    /// The originating ref's marker id (e.g. `cite_ref-ety_1-0`), so a verdict is
    /// addressable back to the human-facing citation on the page. Empty for the
    /// standalone single-claim path, which has no page ref; the page orchestrator
    /// stamps it. Back-compatible (ADR-0009 replay).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ref_id: String,
    /// The claim sentence this verdict judged, echoed so the report is
    /// self-contained — a renderer can show `Claim: … → <verdict>` without
    /// re-reading the page. Empty for the standalone path, where the caller
    /// already holds the claim. Back-compatible (ADR-0009 replay).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub claim: String,
    /// The preceding-sentence context passed to the verifier for co-reference
    /// (SIDE-style), echoed so the report can show what the claim was read
    /// against. Empty when there was no context. Back-compatible.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preceding_context: Vec<String>,
    /// Set when this verdict was produced against an **archive** fallback because
    /// the citation's live `url` was `SourceUnavailable`: the (unreachable) live
    /// URL the archive stands in for. `None` for a verdict from the primary
    /// source. Lets the report say "supported via archive of `<live url>`, which
    /// needs repair" without sniffing `provenance.url`'s hostname. Back-compatible
    /// (ADR-0009 replay).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive_of: Option<url::Url>,
    /// Set when this verdict was produced against an Internet Archive
    /// search-inside snippet (PRD-0009 Layer 2): which scan and scanned page.
    /// `None` for every web-source finding. Back-compatible (ADR-0009 replay).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub book_scan: Option<BookScanProvenance>,
    /// Schema version (Art. 9.1).
    pub schema_version: u32,
}

/// One panel member's parsed verdict plus the fingerprint of the call that produced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelVerdict {
    /// The fingerprint of the model invocation (ADR-0006 Decision 8).
    pub invocation: ModelInvocation,
    /// The parsed verdict (still ungrounded — the gate re-checks the quote).
    pub parsed: ParsedVerdict,
    /// The bounded repair turn, when one was attempted (SP42#25 layer 3). The repair fixes
    /// *transcription* only — the vote's verdict is never re-litigated by it.
    pub repair: Option<RepairAttempt>,
}

/// The outcome of one bounded repair turn (SP42#25 layer 3): the fingerprint of the repair
/// call plus the span it returned (`None` for `NO_SPAN` / unparseable). The span is still
/// ungrounded — the gate re-locates it like any other quote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepairAttempt {
    /// The fingerprint of the repair call (ADR-0006 Decision 8).
    pub invocation: ModelInvocation,
    /// The repaired candidate span, or `None` when the model returned `NO_SPAN`.
    pub quote: Option<String>,
}

/// A persisted per-model vote (ADR-0009 §3): the invocation fingerprint, its returned
/// verdict, and any located passage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelVote {
    /// The fingerprint of the call that cast this vote.
    pub invocation: ModelInvocation,
    /// Its returned verdict.
    pub verdict: CitationVerdict,
    /// Its located passage, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub located_passage: Option<LocatedPassage>,
    /// The raw quote the model claimed, kept regardless of whether it located — the audit/
    /// replay record (ADR-0006 Decision 8). `None` when the model returned no quote.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_quote: Option<String>,
    /// The span the bounded repair turn returned, when one ran (SP42#25 layer 3); the
    /// original `claimed_quote` is never rewritten by a repair.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repaired_quote: Option<String>,
    /// The fingerprint of the repair call, when one ran — recorded even for a `NO_SPAN`
    /// outcome, so every model call stays in the audit record (ADR-0006 Decision 8).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repair_invocation: Option<ModelInvocation>,
}

/// The full result of verifying one use-site: the surfaced finding plus the per-model
/// votes (for the storage record, ADR-0009).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationOutcome {
    /// The surfaced read-only finding.
    pub finding: CitationFinding,
    /// Every per-model vote (with its invocation fingerprint).
    pub votes: Vec<ModelVote>,
}

/// Prepared per-model inputs: the claim plus the *fetched* source body, URL, and optional
/// metadata sidecar. (The fetched body is not on [`CitationVerificationRequest`].)
#[derive(Debug, Clone, Copy)]
pub struct VerifyModelInputs<'a> {
    /// The claim to verify.
    pub claim: &'a str,
    /// The fetched source body text (the grounded bytes).
    pub source_text: &'a str,
    /// The source URL (for prompt display).
    pub source_url: &'a str,
    /// The optional bibliographic metadata sidecar (context only — never grounded).
    pub metadata: Option<&'a CitoidMetadata>,
    /// The optional co-reference context window (context only — never grounded).
    pub context: Option<&'a ClaimContext>,
}

/// Options for a verification run.
#[derive(Debug, Clone)]
pub struct VerifyOptions {
    /// Whether to fetch the Citoid metadata sidecar (best-effort).
    pub include_metadata: bool,
    /// Maximum concurrent model calls.
    pub concurrency: usize,
    /// Sampling / reasoning parameters for each model call.
    pub params: SamplingParams,
    /// Whether to run the bounded repair turn (SP42#25 layer 3): one extra call per
    /// support-class vote whose quote failed to locate, asking for the exact shortest
    /// verbatim span (or `NO_SPAN`). Transcription only — never re-litigates the verdict.
    pub repair_turn: bool,
    /// Pre-fetched source body. When `Some`, `verify_citation_use_site` uses it
    /// instead of fetching — lets the page orchestrator fetch each distinct URL
    /// once. `None` (the default) preserves the byte-identical single-claim path.
    pub prefetched: Option<FetchedSource>,
    /// Pre-supplied bibliographic metadata sidecar. When `Some`, it is used as the
    /// verification-prompt context instead of fetching Citoid — lets a caller pass
    /// known metadata for a reproducible, network-free run. Takes precedence over
    /// `include_metadata`; `None` (the default) preserves the fetch-or-skip behavior.
    pub metadata_sidecar: Option<CitoidMetadata>,
}

impl Default for VerifyOptions {
    fn default() -> Self {
        Self {
            include_metadata: false,
            concurrency: 3,
            params: SamplingParams::deterministic(),
            repair_turn: true,
            prefetched: None,
            metadata_sidecar: None,
        }
    }
}

/// SHA-256 of `bytes` as lowercase hex (64 chars) — the content-addressing identity.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn build_verify_completion_request(
    model: &ModelRef,
    params: &SamplingParams,
    inputs: VerifyModelInputs<'_>,
) -> Result<(ModelCompletionRequest, ModelInvocation), CitationVerificationError> {
    if inputs.claim.trim().is_empty() {
        return Err(CitationVerificationError::InvalidRequest {
            message: "claim is empty".to_string(),
        });
    }
    if inputs.source_text.trim().is_empty() {
        return Err(CitationVerificationError::InvalidRequest {
            message: "source text is empty".to_string(),
        });
    }

    let messages = build_verify_prompt(
        inputs.claim,
        inputs.source_text,
        inputs.source_url,
        inputs.metadata,
        inputs.context,
    )
    .to_vec();
    let prompt_hash = sha256_hex(&serde_json::to_vec(&messages)?);
    let request = ModelCompletionRequest {
        model: model.clone(),
        messages,
        params: params.clone(),
    };
    let invocation = ModelInvocation {
        model: model.clone(),
        quant: None,
        params: params.fingerprint(),
        prompt_hash,
    };
    Ok((request, invocation))
}

fn source_unavailable_model_vote(
    model: &ModelRef,
    params: &SamplingParams,
    inputs: VerifyModelInputs<'_>,
) -> Result<ModelVerdict, CitationVerificationError> {
    let (_request, invocation) = build_verify_completion_request(model, params, inputs)?;
    Ok(ModelVerdict {
        invocation,
        parsed: ParsedVerdict {
            verdict: Verdict::SourceUnavailable,
            quote: None,
        },
        repair: None,
    })
}

/// Run one model's verification over the provider-agnostic [`ModelClient`] boundary,
/// returning its parsed verdict plus the invocation fingerprint.
///
/// An unrecoverable model response defaults to *not supported* (the validate gate), never
/// a support judgment (ADR-0008 §3).
///
/// # Errors
///
/// Returns [`CitationVerificationError`] if the claim/source text is empty, a
/// serialization error, or the model client fails.
pub async fn execute_citation_verify<M>(
    model_client: &M,
    model: &ModelRef,
    params: &SamplingParams,
    inputs: VerifyModelInputs<'_>,
) -> Result<ModelVerdict, CitationVerificationError>
where
    M: ModelClient + ?Sized,
{
    let (request, invocation) = build_verify_completion_request(model, params, inputs)?;
    let completion = model_client.complete(&request).await.map_err(|error| {
        CitationVerificationError::InvalidResponse {
            message: error.to_string(),
        }
    })?;
    let parsed = parse_verdict_response(&completion.text).unwrap_or(ParsedVerdict {
        verdict: Verdict::NotSupported,
        quote: None,
    });
    Ok(ModelVerdict {
        invocation,
        parsed,
        repair: None,
    })
}

/// Run one bounded repair turn (SP42#25 layer 3) over the [`ModelClient`] boundary: hand
/// the model its non-locating quote and the source again, asking for the exact shortest
/// verbatim span (or `NO_SPAN`). The returned span is still ungrounded — the caller
/// re-locates it deterministically like any other quote.
///
/// # Errors
///
/// Returns [`CitationVerificationError`] on a serialization error or model client failure.
pub async fn execute_citation_repair<M>(
    model_client: &M,
    model: &ModelRef,
    params: &SamplingParams,
    inputs: VerifyModelInputs<'_>,
    failed_quote: &str,
) -> Result<RepairAttempt, CitationVerificationError>
where
    M: ModelClient + ?Sized,
{
    let messages = build_repair_prompt(
        inputs.claim,
        inputs.source_text,
        inputs.source_url,
        failed_quote,
    )
    .to_vec();
    let prompt_hash = sha256_hex(&serde_json::to_vec(&messages)?);
    let request = ModelCompletionRequest {
        model: model.clone(),
        messages,
        params: params.clone(),
    };
    let completion = model_client.complete(&request).await.map_err(|error| {
        CitationVerificationError::InvalidResponse {
            message: error.to_string(),
        }
    })?;
    Ok(RepairAttempt {
        invocation: ModelInvocation {
            model: model.clone(),
            quant: None,
            params: params.fingerprint(),
            prompt_hash,
        },
        quote: parse_repair_response(&completion.text),
    })
}

/// Locate a vote's supporting span in `source_text`: the original claimed quote first,
/// then the repaired span (SP42#25 layer 3). Either way the span must string-locate — a
/// repair is a second chance at transcription, never a bypass around the gate.
fn locate_vote_quote(vote: &ModelVerdict, source_text: &str) -> Option<(String, usize)> {
    let original =
        vote.parsed.quote.as_ref().and_then(|quote| {
            locate_quote(quote, source_text).map(|offset| (quote.clone(), offset))
        });
    original.or_else(|| {
        let repaired = vote.repair.as_ref()?.quote.as_ref()?;
        locate_quote(repaired, source_text).map(|offset| (repaired.clone(), offset))
    })
}

/// Fuzzy-locate a vote's supporting span (SP42#25 layer 5): the original claimed quote
/// first, then the repaired one. Only reached after exact locate failed for every
/// winner-class vote; the returned span is the source's own text.
fn fuzzy_locate_vote_quote(
    vote: &ModelVerdict,
    source_text: &str,
) -> Option<super::locate_quote::FuzzyLocate> {
    let original = vote
        .parsed
        .quote
        .as_ref()
        .and_then(|quote| locate_quote_fuzzy(quote, source_text));
    original.or_else(|| {
        let repaired = vote.repair.as_ref()?.quote.as_ref()?;
        locate_quote_fuzzy(repaired, source_text)
    })
}

/// Assemble the final [`CitationFinding`] from the panel's model verdicts (SP42#25 layer 6).
///
/// Votes the panel; the surfaced `verdict` is the panel's *judgment* and is never rewritten.
/// For a `Supported`/`Partial` winner it re-locates a winning-class quote in `source_text`:
/// if one locates the finding is `grounding_status: Located` with the passage; if none does,
/// the verdict STAYS as the panel judged it but is marked `Unlocated` (unverified) — honest
/// for the human-in-the-loop consumer, with anti-fabrication enforced at the action gate
/// ([`is_groundable_support`]), never by rewriting the verdict (refines ADR-0007 §5). A
/// no-quote (non-support) winner is `NotApplicable`.
#[must_use]
pub fn assemble_citation_finding(
    source_text: &str,
    provenance: &SourceProvenance,
    votes: &[ModelVerdict],
    use_site_ordinal: u32,
) -> CitationFinding {
    let verdicts: Vec<Verdict> = votes.iter().map(|vote| vote.parsed.verdict).collect();
    let Some(vote) = n_class_vote(&verdicts) else {
        return no_quote_finding(
            CitationVerdict::SourceUnavailable,
            GroundingStatus::NotApplicable,
            PanelAgreement::new(0, 0),
            provenance,
            use_site_ordinal,
        );
    };

    if vote.winner.is_support_class() {
        let winners = || {
            votes
                .iter()
                .filter(|candidate| candidate.parsed.verdict == vote.winner)
        };
        // Exact locate first (original quote, then the repaired one — layer 3); only when
        // every winner-class quote misses, fall back to the guarded fuzzy match (layer 5),
        // whose passage is the source's own text.
        let located = winners()
            .find_map(|candidate| {
                locate_vote_quote(candidate, source_text)
                    .map(|(quote, offset)| (quote, offset, GroundingStatus::Located))
            })
            .or_else(|| {
                winners().find_map(|candidate| {
                    fuzzy_locate_vote_quote(candidate, source_text)
                        .map(|hit| (hit.span, hit.offset, GroundingStatus::LocatedFuzzy))
                })
            });

        return match located {
            Some((quote, offset, grounding_status)) => {
                let verdict = CitationVerdict::from(vote.winner);
                CitationFinding {
                    kind: CitationFindingKind::CitationVerdict,
                    verdict,
                    grounding_status,
                    agreement: vote.agreement,
                    passage: Some(LocatedPassage {
                        quote: quote.clone(),
                        offset,
                    }),
                    source_unavailable_reason: derive_source_unavailable_reason(
                        verdict, provenance,
                    ),
                    unusable_reason: None,
                    provenance: provenance.clone(),
                    source_excerpt: None,
                    metadata: None,
                    grounding: GroundingAssertion::LocatedQuote {
                        quote,
                        source_hash: provenance.content_hash.clone(),
                        offset,
                    },
                    use_site_ordinal,
                    ref_id: String::new(),
                    claim: String::new(),
                    preceding_context: Vec::new(),
                    archive_of: None,
                    book_scan: None,
                    schema_version: SCHEMA_VERSION,
                }
            }
            // Layer 6: the support quote did not locate. Do NOT rewrite the verdict — surface
            // the panel's judgment marked `Unlocated` (unverified). Anti-fabrication is the
            // action gate's job ([`is_groundable_support`]), not a silent downgrade.
            None => no_quote_finding(
                CitationVerdict::from(vote.winner),
                GroundingStatus::Unlocated,
                vote.agreement,
                provenance,
                use_site_ordinal,
            ),
        };
    }

    no_quote_finding(
        CitationVerdict::from(vote.winner),
        GroundingStatus::NotApplicable,
        vote.agreement,
        provenance,
        use_site_ordinal,
    )
}

/// Whether a finding is a CONFIRMED support verdict — the *only* gate an autonomous
/// accept/edit path may use (SP42#25 layer 6). `true` iff the verdict is support-class AND
/// its supporting quote located in the fetched source (`grounding_status: Located`). A human
/// reviewer may still weigh an `Unlocated` support; an autonomous path may not.
#[must_use]
pub fn is_groundable_support(finding: &CitationFinding) -> bool {
    finding.grounding_status == GroundingStatus::Located
        && matches!(
            finding.verdict,
            CitationVerdict::Judged(SupportLevel::Supported | SupportLevel::Partial)
        )
}

/// Build the per-model vote records for the verdict store (ADR-0009): each vote's
/// invocation fingerprint, its verdict, and — for a support-class vote whose quote
/// locates — its located passage.
#[must_use]
pub fn build_model_votes(votes: &[ModelVerdict], source_text: &str) -> Vec<ModelVote> {
    votes
        .iter()
        .map(|vote| {
            let located_passage = if vote.parsed.verdict.is_support_class() {
                locate_vote_quote(vote, source_text)
                    .map(|(quote, offset)| LocatedPassage { quote, offset })
            } else {
                None
            };
            ModelVote {
                invocation: vote.invocation.clone(),
                verdict: CitationVerdict::from(vote.parsed.verdict),
                located_passage,
                claimed_quote: vote.parsed.quote.clone(),
                repaired_quote: vote
                    .repair
                    .as_ref()
                    .and_then(|attempt| attempt.quote.clone()),
                repair_invocation: vote
                    .repair
                    .as_ref()
                    .map(|attempt| attempt.invocation.clone()),
            }
        })
        .collect()
}

/// Derive the reason a `SourceUnavailable` verdict was reached from the verdict and
/// provenance. Returns `None` for any other verdict.
fn derive_source_unavailable_reason(
    verdict: CitationVerdict,
    provenance: &SourceProvenance,
) -> Option<SourceUnavailableReason> {
    if !matches!(verdict, CitationVerdict::SourceUnavailable) {
        return None;
    }
    match provenance.http_status {
        Some(status) if (200..=299).contains(&status) => Some(SourceUnavailableReason::Unusable),
        _ => Some(SourceUnavailableReason::Unreachable),
    }
}

/// A finding with no located passage, grounded on "the source was fetched".
fn no_quote_finding(
    verdict: CitationVerdict,
    grounding_status: GroundingStatus,
    agreement: PanelAgreement,
    provenance: &SourceProvenance,
    use_site_ordinal: u32,
) -> CitationFinding {
    CitationFinding {
        kind: CitationFindingKind::CitationVerdict,
        verdict,
        grounding_status,
        agreement,
        passage: None,
        source_unavailable_reason: derive_source_unavailable_reason(verdict, provenance),
        unusable_reason: None,
        provenance: provenance.clone(),
        source_excerpt: None,
        metadata: None,
        grounding: GroundingAssertion::SourceFetched {
            source_hash: provenance.content_hash.clone(),
        },
        use_site_ordinal,
        ref_id: String::new(),
        claim: String::new(),
        preceding_context: Vec::new(),
        archive_of: None,
        book_scan: None,
        schema_version: SCHEMA_VERSION,
    }
}

/// Longest reviewer-facing source excerpt retained on a finding (chars). Bounds
/// the report size while showing enough context around the located quote.
const SOURCE_EXCERPT_MAX_CHARS: usize = 700;

/// A bounded, reviewer-facing excerpt of the extracted source: a window centred on
/// the located quote (so the reviewer sees it in context), or the head of the body
/// when no quote located. `None` for an empty body. Display only — never grounded.
#[must_use]
pub fn build_source_excerpt(source_text: &str, passage: Option<&LocatedPassage>) -> Option<String> {
    if source_text.trim().is_empty() {
        return None;
    }
    let chars: Vec<char> = source_text.chars().collect();
    let total = chars.len();
    if total <= SOURCE_EXCERPT_MAX_CHARS {
        return Some(source_text.trim().to_string());
    }
    // The passage offset is a byte offset into `source_text`; map it to a char index.
    let center = passage.map_or(0, |passage| {
        source_text
            .char_indices()
            .take_while(|(byte, _)| *byte < passage.offset)
            .count()
    });
    let half = SOURCE_EXCERPT_MAX_CHARS / 2;
    let mut start = center.saturating_sub(half);
    let end = (start + SOURCE_EXCERPT_MAX_CHARS).min(total);
    start = end.saturating_sub(SOURCE_EXCERPT_MAX_CHARS);
    let body: String = chars[start..end].iter().collect();
    let body = body.trim();
    let mut excerpt = String::new();
    if start > 0 {
        excerpt.push_str("… ");
    }
    excerpt.push_str(body);
    if end < total {
        excerpt.push_str(" …");
    }
    Some(excerpt)
}

/// Build a [`VerificationOutcome`] for an unusable source (short-circuit, no model call).
/// This captures the pattern: source was fetched but marked unusable by the deterministic
/// body-usability gate. The finding records the reason and marks grounding as [`GroundingStatus::NotApplicable`].
fn unusable_source_outcome(
    usability_reason: BodyUsabilityReason,
    provenance: &SourceProvenance,
    use_site_ordinal: u32,
) -> VerificationOutcome {
    let mut finding = no_quote_finding(
        CitationVerdict::SourceUnavailable,
        GroundingStatus::NotApplicable,
        PanelAgreement::new(0, 0),
        provenance,
        use_site_ordinal,
    );
    // Record the specific body-usability reason only for sources that were actually
    // fetched (2xx → `Unusable`). A non-2xx response is `Unreachable`, not
    // fetched-but-unusable, so its empty body must not be mis-tagged (e.g. `ShortBody`);
    // keep `unusable_reason` None there, matching the field contract.
    if matches!(
        finding.source_unavailable_reason,
        Some(SourceUnavailableReason::Unusable)
    ) {
        finding.unusable_reason = Some(usability_reason);
    }
    VerificationOutcome {
        finding,
        votes: Vec::new(),
    }
}

/// A no-model, no-quote `SourceUnavailable` outcome — the book lane's
/// short-circuit when a scan cannot be used at all (PRD-0009 Layer 2 /
/// ADR-0018 Decision 4: no exact-edition scan, no full-text index, or the
/// item metadata was unreachable). Unreachable-vs-unusable is derived from
/// `provenance.http_status` exactly like the fetched-web path (`0` →
/// unreachable, 2xx → unusable).
#[must_use]
pub fn book_scan_unavailable_outcome(
    provenance: &SourceProvenance,
    use_site_ordinal: u32,
) -> VerificationOutcome {
    VerificationOutcome {
        finding: no_quote_finding(
            CitationVerdict::SourceUnavailable,
            GroundingStatus::NotApplicable,
            PanelAgreement::new(0, 0),
            provenance,
            use_site_ordinal,
        ),
        votes: Vec::new(),
    }
}

/// The searched-and-found-nothing outcome (PRD-0009 resolved Q4 / ADR-0018
/// Decision 4): the scan **is** indexed and search-inside ran, but returned
/// zero snippets after both the cited-page and whole-book passes. That is
/// `not_supported`, not `SourceUnavailable` — the source exists and was
/// searched; it yielded no supporting passage. Deliberately a verdict without
/// a model panel: the judgment IS the deterministic search outcome, grounded
/// by the fetched search response (`SourceFetched` provenance), and it never
/// fabricates a passage (`NotApplicable` grounding, no quote).
#[must_use]
pub fn book_searched_not_supported_outcome(
    provenance: &SourceProvenance,
    use_site_ordinal: u32,
) -> VerificationOutcome {
    VerificationOutcome {
        finding: no_quote_finding(
            CitationVerdict::Judged(SupportLevel::NotSupported),
            GroundingStatus::NotApplicable,
            PanelAgreement::new(0, 0),
            provenance,
            use_site_ordinal,
        ),
        votes: Vec::new(),
    }
}

/// A fetched source body plus the HTTP status it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedSource {
    pub text: String,
    pub status: u16,
    pub content_type: String,
    /// Pre-extraction HTML body, present only for HTML responses. Needed at the
    /// usability gate for structured paywall markers; consumed there and not
    /// retained downstream (grounding uses `text`).
    pub raw_html: Option<String>,
    /// `true` only for a body assembled from Internet Archive search-inside
    /// snippets by the book-grounding path (PRD-0009 Layer 2). Provenance for
    /// the usability gate: a snippet body bypasses **only** the generic
    /// short-body floor (ADR-0018 Decision 4) — arbitrary short web pages
    /// still short-circuit. Never set by [`fetch_source`].
    pub book_snippet: bool,
}

/// Fetch a source body (read-only GET), extracting text from HTML and recovering past a
/// surviving Wayback banner. A non-2xx/3xx yields empty text (→ `SourceUnavailable`).
///
/// # Errors
///
/// Returns [`CitationVerificationError::InvalidRequest`] if `source_url` does not parse, or
/// [`CitationVerificationError::InvalidResponse`] if the injected client's request fails.
pub async fn fetch_source<C>(
    client: &C,
    source_url: &str,
) -> Result<FetchedSource, CitationVerificationError>
where
    C: HttpClient + ?Sized,
{
    let fetch_url = rewrite_wayback_url(source_url);
    let url: Url = fetch_url
        .parse()
        .map_err(|_| CitationVerificationError::InvalidRequest {
            message: format!("invalid source url {source_url:?}"),
        })?;
    let response = client
        .execute(HttpRequest {
            method: HttpMethod::Get,
            url,
            headers: BTreeMap::new(),
            body: Vec::new(),
        })
        .await
        .map_err(|error| CitationVerificationError::InvalidResponse {
            message: error.to_string(),
        })?;
    if !(200..400).contains(&response.status) {
        return Ok(FetchedSource {
            text: String::new(),
            status: response.status,
            content_type: String::new(),
            raw_html: None,
            book_snippet: false,
        });
    }
    let content_type = response
        .headers
        .get("content-type")
        .cloned()
        .unwrap_or_default();
    let body = String::from_utf8_lossy(&response.body).into_owned();
    let is_html = looks_like_html(&content_type, &body);
    let (extracted, raw_html) = if is_html {
        (html_to_text(&body), Some(body))
    } else {
        (body, None)
    };
    Ok(FetchedSource {
        text: recover_wayback_body(&extracted),
        status: response.status,
        content_type,
        raw_html,
        book_snippet: false,
    })
}

/// Best-effort Citoid metadata fetch; any failure yields `None` (never blocks).
async fn fetch_metadata<C>(client: &C, source_url: &str) -> Option<CitoidMetadata>
where
    C: HttpClient + ?Sized,
{
    let response = client
        .execute(build_citoid_request(source_url))
        .await
        .ok()?;
    if !(200..300).contains(&response.status) {
        return None;
    }
    let raw = parse_citoid_response(&response.body)?;
    build_citoid_header(&raw, source_url)
}

/// Run the bounded repair turn (SP42#25 layer 3): one extra call per support-class
/// vote whose quote failed to locate, updating those votes in place. Best-effort —
/// a failed repair call leaves the vote as-is.
async fn run_repair_turn<M>(
    model_client: &M,
    params: &SamplingParams,
    concurrency: usize,
    inputs: VerifyModelInputs<'_>,
    source_text: &str,
    model_verdicts: &mut [ModelVerdict],
) where
    M: ModelClient + ?Sized,
{
    let pending: Vec<(usize, ModelRef, String)> = model_verdicts
        .iter()
        .enumerate()
        .filter_map(|(index, vote)| {
            if !vote.parsed.verdict.is_support_class() {
                return None;
            }
            let quote = vote.parsed.quote.as_ref()?;
            if locate_quote(quote, source_text).is_some() {
                return None;
            }
            Some((index, vote.invocation.model.clone(), quote.clone()))
        })
        .collect();
    let repairs = map_with_concurrency(
        pending,
        concurrency,
        |(index, model, failed_quote), _| async move {
            let attempt =
                execute_citation_repair(model_client, &model, params, inputs, &failed_quote).await;
            (index, attempt)
        },
    )
    .await;
    for (index, attempt) in repairs {
        if let Ok(attempt) = attempt {
            model_verdicts[index].repair = Some(attempt);
        }
    }
}

/// Verify one (claim, source) use-site end-to-end (ADR-0008 §3, ADR-0007).
///
/// Fetches the source once over the injected `HttpClient`, runs the deterministic
/// body-usability gate (short-circuiting to `SourceUnavailable` with **no model call**),
/// then fans the panel out over the [`ModelClient`] with bounded concurrency and assembles
/// the grounded finding plus the per-model votes. Performs only read-only requests.
///
/// # Errors
///
/// Returns [`CitationVerificationError`] for an empty panel or an unfetchable source URL.
/// Individual model failures are recorded as `SourceUnavailable` panel votes so the
/// configured panel size is preserved in the audit trail and agreement counts.
// The injected edges (fetch/model/clock), the panel, the request, its optional context
// window, the use-site ordinal, and the run options are all distinct, named inputs; bundling
// them would obscure rather than clarify. The context rides a separate argument by design —
// it is kept off `CitationVerificationRequest` (the clean claim+url record, ADR-0008 §1).
#[allow(clippy::too_many_arguments)]
pub async fn verify_citation_use_site<C, M>(
    fetch_client: &C,
    model_client: &M,
    clock: &dyn Clock,
    panel: &[ModelRef],
    request: &CitationVerificationRequest,
    context: Option<&ClaimContext>,
    use_site_ordinal: u32,
    options: VerifyOptions,
) -> Result<VerificationOutcome, CitationVerificationError>
where
    C: HttpClient + ?Sized,
    M: ModelClient + ?Sized,
{
    if panel.is_empty() {
        return Err(CitationVerificationError::InvalidRequest {
            message: "model panel is empty".to_string(),
        });
    }

    let fetched = match &options.prefetched {
        Some(source) => source.clone(),
        None => fetch_source(fetch_client, request.source_url.as_str()).await?,
    };
    let provenance = SourceProvenance {
        url: request.source_url.clone(),
        content_hash: sha256_hex(fetched.text.as_bytes()),
        fetched_at: clock.now_ms(),
        http_status: Some(fetched.status),
    };

    let body = if fetched.text.is_empty() {
        None
    } else {
        Some(fetched.text.as_str())
    };
    let mut usability = classify_source_usability(
        request.source_url.as_str(),
        &fetched.content_type,
        fetched.raw_html.as_deref(),
        body,
    );
    // ADR-0018 Decision 4: a search-inside snippet body bypasses ONLY the
    // generic short-body floor — a verbatim OCR snippet is a valid grounding
    // body well below the web-page floor. Every other unusability reason
    // still applies, and an empty snippet (`body == None`) stays unusable.
    if fetched.book_snippet
        && body.is_some()
        && usability.reason == super::body_classifier::BodyUsabilityReason::ShortBody
    {
        usability = super::body_classifier::BodyUsability {
            usable: true,
            reason: super::body_classifier::BodyUsabilityReason::Ok,
        };
    }
    if !usability.usable {
        return Ok(unusable_source_outcome(
            usability.reason,
            &provenance,
            use_site_ordinal,
        ));
    }

    // Citoid metadata is verification-prompt context only (never grounded, never
    // attached to the finding here). The page report's *display* metadata is fetched
    // separately, deduped and paced, by the server route.
    let metadata = if let Some(sidecar) = options.metadata_sidecar.clone() {
        Some(sidecar)
    } else if options.include_metadata {
        fetch_metadata(fetch_client, request.source_url.as_str()).await
    } else {
        None
    };
    let inputs = VerifyModelInputs {
        claim: &request.claim,
        source_text: &fetched.text,
        source_url: request.source_url.as_str(),
        metadata: metadata.as_ref(),
        context,
    };

    let params = &options.params;
    let concurrency = options.concurrency.max(1);
    let results = map_with_concurrency(panel.to_vec(), concurrency, |model, _index| async move {
        match execute_citation_verify(model_client, &model, params, inputs).await {
            Ok(verdict) => Ok(verdict),
            Err(CitationVerificationError::InvalidResponse { .. }) => {
                source_unavailable_model_vote(&model, params, inputs)
            }
            Err(error) => Err(error),
        }
    })
    .await;
    let mut model_verdicts: Vec<ModelVerdict> = results.into_iter().collect::<Result<_, _>>()?;

    // Bounded repair turn (SP42#25 layer 3): one extra call per support-class vote whose
    // quote failed to locate. Best-effort — a failed repair call leaves the vote as-is.
    if options.repair_turn {
        run_repair_turn(
            model_client,
            params,
            concurrency,
            inputs,
            &fetched.text,
            &mut model_verdicts,
        )
        .await;
    }

    let mut finding = assemble_citation_finding(
        &fetched.text,
        &provenance,
        &model_verdicts,
        use_site_ordinal,
    );
    // Attach a bounded excerpt of what the panel read (windowed on the located
    // quote) — reviewer context the surfaced finding does not otherwise carry.
    // Source metadata is attached separately by the server route (deduped + paced).
    finding.source_excerpt = build_source_excerpt(&fetched.text, finding.passage.as_ref());
    let votes = build_model_votes(&model_verdicts, &fetched.text);
    Ok(VerificationOutcome { finding, votes })
}

#[cfg(test)]
mod excerpt_tests {
    use super::{LocatedPassage, build_source_excerpt};

    #[test]
    fn empty_body_has_no_excerpt() {
        assert_eq!(build_source_excerpt("   ", None), None);
    }

    #[test]
    fn short_body_is_returned_whole() {
        assert_eq!(
            build_source_excerpt("a short source body", None).as_deref(),
            Some("a short source body")
        );
    }

    #[test]
    fn long_body_windows_around_the_located_quote() {
        // A long body with a marker sentence two-thirds in; the excerpt should be
        // bounded, ellipsised on both ends, and contain the quoted passage.
        let body = format!("{}MARKER PASSAGE HERE{}", "x".repeat(900), "y".repeat(900));
        let offset = body.find("MARKER").expect("marker present");
        let passage = LocatedPassage {
            quote: "MARKER PASSAGE HERE".to_string(),
            offset,
        };
        let excerpt = build_source_excerpt(&body, Some(&passage)).expect("excerpt");
        assert!(excerpt.contains("MARKER PASSAGE HERE"));
        assert!(excerpt.starts_with('…') && excerpt.ends_with('…'));
        assert!(excerpt.chars().count() < body.chars().count());
    }

    #[test]
    fn long_body_without_a_quote_shows_the_head() {
        let body = "z".repeat(2000);
        let excerpt = build_source_excerpt(&body, None).expect("excerpt");
        assert!(
            !excerpt.starts_with('…'),
            "head excerpt has no leading ellipsis"
        );
        assert!(excerpt.ends_with('…'), "trailing ellipsis when truncated");
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use futures::executor::block_on;
    use proptest::prelude::*;
    use sp42_types::{
        ModelClientError, ModelCompletion, ModelInvocation, ModelRef, SamplingParams,
        StubModelClient,
    };

    use super::{
        BodyUsabilityReason, CitationFinding, CitationVerificationRequest, ClaimContext,
        GroundingAssertion, GroundingStatus, ModelVerdict, RepairAttempt, SourceProvenance,
        VerifyModelInputs, VerifyOptions, assemble_citation_finding, build_model_votes,
        execute_citation_repair, execute_citation_verify, is_groundable_support,
        verify_citation_use_site,
    };
    use crate::citation::parsing::ParsedVerdict;
    use crate::citation::verdict::{CitationVerdict, SupportLevel, Verdict};
    use crate::traits::{FixedClock, StubHttpClient};
    use crate::types::HttpResponse;

    fn model() -> ModelRef {
        ModelRef::new("openrouter", "test-model", "test-model")
    }

    fn params() -> SamplingParams {
        SamplingParams::deterministic()
    }

    fn inputs<'a>(claim: &'a str, source: &'a str) -> VerifyModelInputs<'a> {
        VerifyModelInputs {
            claim,
            source_text: source,
            source_url: "https://example.com",
            metadata: None,
            context: None,
        }
    }

    fn completion(text: &str) -> ModelCompletion {
        ModelCompletion {
            text: text.to_string(),
            served_model: None,
        }
    }

    fn model_transport_failure() -> ModelClientError {
        ModelClientError::Transport {
            message: "model timed out".to_string(),
        }
    }

    fn provenance() -> SourceProvenance {
        SourceProvenance {
            url: "https://example.com".parse().expect("url"),
            content_hash: "deadbeef".to_string(),
            fetched_at: 42,
            http_status: Some(200),
        }
    }

    #[test]
    fn provenance_without_http_status_deserializes_to_none() {
        // ADR-0009 replay: a snapshot written before `http_status` existed must still load.
        let legacy = r#"{"url":"https://example.com/","content_hash":"deadbeef","fetched_at":42}"#;
        let provenance: SourceProvenance =
            serde_json::from_str(legacy).expect("legacy provenance deserializes");
        assert_eq!(provenance.http_status, None);
        assert_eq!(provenance.fetched_at, 42);
    }

    fn model_verdict(verdict: Verdict, quote: Option<&str>) -> ModelVerdict {
        ModelVerdict {
            invocation: ModelInvocation {
                model: model(),
                quant: None,
                params: BTreeMap::new(),
                prompt_hash: "test".to_string(),
            },
            parsed: ParsedVerdict {
                verdict,
                quote: quote.map(ToString::to_string),
            },
            repair: None,
        }
    }

    fn repaired_verdict(
        verdict: Verdict,
        quote: Option<&str>,
        repair_quote: Option<&str>,
    ) -> ModelVerdict {
        let mut vote = model_verdict(verdict, quote);
        vote.repair = Some(RepairAttempt {
            invocation: ModelInvocation {
                model: model(),
                quant: None,
                params: BTreeMap::new(),
                prompt_hash: "repair".to_string(),
            },
            quote: repair_quote.map(ToString::to_string),
        });
        vote
    }

    #[test]
    fn execute_runs_through_the_model_client_and_fingerprints() {
        let client = StubModelClient::new([Ok(completion(
            r#"{"verdict": "PARTIAL", "quote": "it is believed"}"#,
        ))]);
        let verdict = block_on(execute_citation_verify(
            &client,
            &model(),
            &params(),
            inputs(
                "the treaty was signed in Paris",
                "It is believed the treaty was signed in Paris.",
            ),
        ))
        .expect("executes");
        assert_eq!(verdict.parsed.verdict, Verdict::Partial);
        // The invocation is fingerprinted: a sha256 prompt hash + the sampling params.
        assert_eq!(verdict.invocation.prompt_hash.len(), 64);
        assert!(
            verdict
                .invocation
                .prompt_hash
                .bytes()
                .all(|b| b.is_ascii_hexdigit())
        );
        assert_eq!(
            verdict
                .invocation
                .params
                .get("temperature")
                .map(String::as_str),
            Some("0")
        );
        assert_eq!(verdict.invocation.model, model());
    }

    #[test]
    fn execute_defaults_unrecoverable_content_to_not_supported() {
        let client = StubModelClient::new([Ok(completion("i could not tell you, honestly"))]);
        let verdict = block_on(execute_citation_verify(
            &client,
            &model(),
            &params(),
            inputs("a claim", "a usable source body"),
        ))
        .expect("executes");
        assert_eq!(verdict.parsed.verdict, Verdict::NotSupported);
        assert_eq!(verdict.parsed.quote, None);
    }

    #[test]
    fn execute_propagates_a_model_client_failure() {
        let client = StubModelClient::new([]); // empty queue -> the stub errors
        let result = block_on(execute_citation_verify(
            &client,
            &model(),
            &params(),
            inputs("a claim", "a usable source body"),
        ));
        assert!(result.is_err());
    }

    #[test]
    fn assemble_grounds_a_supported_verdict_with_a_locatable_quote() {
        let source = "Acme Corp was established in 1985 by its founder John Smith.";
        let finding = assemble_citation_finding(
            source,
            &provenance(),
            &[model_verdict(
                Verdict::Supported,
                Some("established in 1985"),
            )],
            7,
        );
        assert_eq!(
            finding.verdict,
            CitationVerdict::Judged(SupportLevel::Supported)
        );
        assert_eq!(finding.grounding_status, GroundingStatus::Located);
        assert!(is_groundable_support(&finding));
        assert_eq!(finding.use_site_ordinal, 7);
        assert!(matches!(
            finding.grounding,
            GroundingAssertion::LocatedQuote { .. }
        ));
        assert!(finding.passage.is_some());
    }

    #[test]
    fn assemble_marks_an_unlocatable_support_as_unverified_not_downgraded() {
        // Layer 6: a support quote that does not locate is NOT downgraded — the verdict
        // stays as the panel judged it, marked `Unlocated` (unverified) and NOT groundable.
        let source = "Acme Corp was established in 1985.";
        let finding = assemble_citation_finding(
            source,
            &provenance(),
            &[model_verdict(
                Verdict::Supported,
                Some("founded in 1772 by Napoleon"),
            )],
            0,
        );
        assert_eq!(
            finding.verdict,
            CitationVerdict::Judged(SupportLevel::Supported)
        );
        assert_eq!(finding.grounding_status, GroundingStatus::Unlocated);
        assert!(!is_groundable_support(&finding));
        assert!(finding.passage.is_none());
        assert!(matches!(
            finding.grounding,
            GroundingAssertion::SourceFetched { .. }
        ));
    }

    #[test]
    fn assemble_breaks_ties_skeptically() {
        let source = "Acme Corp was established in 1985.";
        let finding = assemble_citation_finding(
            source,
            &provenance(),
            &[
                model_verdict(Verdict::Supported, Some("established in 1985")),
                model_verdict(Verdict::NotSupported, None),
            ],
            0,
        );
        assert_eq!(
            finding.verdict,
            CitationVerdict::Judged(SupportLevel::NotSupported)
        );
    }

    #[test]
    fn build_model_votes_carries_fingerprint_quote_and_located_passage() {
        let source = "Acme Corp was established in 1985.";
        let votes = build_model_votes(
            &[
                model_verdict(Verdict::Supported, Some("established in 1985")),
                model_verdict(Verdict::NotSupported, None),
                model_verdict(Verdict::Supported, Some("absent verbatim span")),
            ],
            source,
        );
        assert_eq!(votes.len(), 3);
        assert_eq!(votes[0].invocation.prompt_hash, "test");
        // Quote locates: both the located passage and the raw claimed quote are present.
        assert!(votes[0].located_passage.is_some());
        assert_eq!(
            votes[0].claimed_quote.as_deref(),
            Some("established in 1985")
        );
        // No quote claimed: both none.
        assert!(votes[1].located_passage.is_none());
        assert_eq!(votes[1].claimed_quote, None);
        // KEY (SP42#25): a support quote that does NOT locate is still captured as
        // claimed_quote (so the offline locate-replay harness can see it), even though the
        // gate located nothing.
        assert!(votes[2].located_passage.is_none());
        assert_eq!(
            votes[2].claimed_quote.as_deref(),
            Some("absent verbatim span")
        );
    }

    fn long_html_with(quote: &str) -> Vec<u8> {
        let padding =
            "This is real article prose that gives the body enough length to be usable. ".repeat(8);
        format!("<html><body><p>{padding}{quote}. {padding}</p></body></html>").into_bytes()
    }

    fn request(claim: &str, url: &str) -> CitationVerificationRequest {
        CitationVerificationRequest {
            wiki_id: "enwiki".to_string(),
            rev_id: 1,
            title: "X".to_string(),
            claim: claim.to_string(),
            source_url: url.parse().expect("url"),
        }
    }

    #[test]
    fn end_to_end_supported_outcome_with_votes() {
        let fetch = StubHttpClient::new([Ok(HttpResponse {
            status: 200,
            headers: BTreeMap::from([("content-type".to_string(), "text/html".to_string())]),
            body: long_html_with("the bridge opened in 1998"),
        })]);
        let model_client = StubModelClient::new([Ok(completion(
            r#"{"verdict": "SUPPORTED", "quote": "the bridge opened in 1998"}"#,
        ))]);
        let outcome = block_on(verify_citation_use_site(
            &fetch,
            &model_client,
            &FixedClock::new(1000),
            &[model()],
            &request("The bridge opened in 1998", "https://example.com/bridge"),
            None,
            3,
            VerifyOptions::default(),
        ))
        .expect("verifies");
        assert_eq!(
            outcome.finding.verdict,
            CitationVerdict::Judged(SupportLevel::Supported)
        );
        assert_eq!(outcome.finding.provenance.fetched_at, 1000);
        assert_eq!(outcome.finding.use_site_ordinal, 3);
        assert_eq!(outcome.votes.len(), 1);
        assert_eq!(outcome.votes[0].invocation.prompt_hash.len(), 64);
    }

    #[test]
    fn prefetched_source_skips_http_fetch() {
        // Empty HTTP queue: if the verifier tried to fetch, it would error.
        let http = StubHttpClient::new([]);
        let model_client = StubModelClient::new([Ok(completion(
            r#"{"verdict": "SUPPORTED", "quote": "cats purr and sleep"}"#,
        ))]);
        let request = CitationVerificationRequest {
            wiki_id: "enwiki".into(),
            rev_id: 1,
            title: "Cats".into(),
            claim: "Cats purr.".into(),
            source_url: url::Url::parse("https://example.test/a").unwrap(),
        };
        let mut options = VerifyOptions {
            repair_turn: false,
            ..VerifyOptions::default()
        };
        // Body must be ≥300 chars to pass usability check (SHORT_BODY_FLOOR in body_classifier.rs).
        // The prose padding is repeated to ensure total length exceeds 300 characters.
        let long_body =
            "This is real article prose that gives the body enough length to be usable. ".repeat(8)
                + "cats purr and sleep all day long. "
                + "This is real article prose that gives the body enough length to be usable. ";
        assert!(
            long_body.chars().count() >= 300,
            "test body must be >= 300 chars (SHORT_BODY_FLOOR)"
        );
        options.prefetched = Some(super::FetchedSource {
            text: long_body,
            status: 200,
            content_type: String::new(),
            raw_html: None,
            book_snippet: false,
        });
        let outcome = block_on(verify_citation_use_site(
            &http,
            &model_client,
            &FixedClock::new(0),
            &[model()],
            &request,
            None,
            0,
            options,
        ))
        .expect("verifies from prefetched source");
        // CitationVerdict is `Judged(SupportLevel)` | `SourceUnavailable`
        assert_eq!(
            outcome.finding.verdict,
            CitationVerdict::Judged(SupportLevel::Supported)
        );
    }

    #[test]
    fn metadata_sidecar_reaches_the_prompt_without_fetching() {
        use crate::citation::citoid::CitoidMetadata;

        // Same prefetched body + empty HTTP queue (any fetch — body or Citoid — would error),
        // run once with no metadata and once with a sidecar. The sidecar must (a) avoid a fetch
        // and (b) change the rendered prompt, observable as a different prompt fingerprint.
        fn prompt_hash_with(metadata: Option<CitoidMetadata>) -> String {
            let http = StubHttpClient::new([]);
            let model_client = StubModelClient::new([Ok(completion(
                r#"{"verdict": "SUPPORTED", "quote": "cats purr and sleep"}"#,
            ))]);
            let request = CitationVerificationRequest {
                wiki_id: "enwiki".into(),
                rev_id: 1,
                title: "Cats".into(),
                claim: "Cats purr.".into(),
                source_url: url::Url::parse("https://example.test/a").unwrap(),
            };
            let long_body =
                "This is real article prose that gives the body enough length to be usable. "
                    .repeat(8)
                    + "cats purr and sleep all day long. ";
            let options = VerifyOptions {
                repair_turn: false,
                prefetched: Some(super::FetchedSource {
                    text: long_body,
                    status: 200,
                    content_type: String::new(),
                    raw_html: None,
                    book_snippet: false,
                }),
                metadata_sidecar: metadata,
                ..VerifyOptions::default()
            };
            let outcome = block_on(verify_citation_use_site(
                &http,
                &model_client,
                &FixedClock::new(0),
                &[model()],
                &request,
                None,
                0,
                options,
            ))
            .expect("verifies from prefetched source");
            outcome.votes[0].invocation.prompt_hash.clone()
        }

        let baseline = prompt_hash_with(None);
        let with_sidecar = prompt_hash_with(Some(CitoidMetadata {
            publication: Some("The Daily Example".into()),
            published: None,
            author: None,
            title: None,
            url: "https://example.test/a".into(),
        }));
        assert_ne!(
            baseline, with_sidecar,
            "metadata sidecar must be rendered into the verification prompt"
        );
    }

    #[test]
    fn empty_context_matches_no_context_finding() {
        // The A/B control arm: supplying an empty ClaimContext must produce a finding
        // identical to today's no-context path (the prompt is byte-identical, so the whole
        // outcome is too). Drive the orchestration twice with the same stubbed source/model.
        fn run(context: Option<&ClaimContext>) -> CitationFinding {
            let fetch = StubHttpClient::new([Ok(HttpResponse {
                status: 200,
                headers: BTreeMap::from([("content-type".to_string(), "text/html".to_string())]),
                body: long_html_with("the bridge opened in 1998"),
            })]);
            let model_client = StubModelClient::new([Ok(completion(
                r#"{"verdict": "SUPPORTED", "quote": "the bridge opened in 1998"}"#,
            ))]);
            block_on(verify_citation_use_site(
                &fetch,
                &model_client,
                &FixedClock::new(1000),
                &[model()],
                &request("The bridge opened in 1998", "https://example.com/bridge"),
                context,
                0,
                VerifyOptions::default(),
            ))
            .expect("verifies")
            .finding
        }

        let none = run(None);
        let empty = run(Some(&ClaimContext::default()));
        assert_eq!(none, empty);
    }

    #[test]
    fn quote_only_in_context_does_not_ground() {
        // Structural safety: the grounding gate only ever locates quotes in the source body,
        // so a quote that lives in the context window (never passed here) cannot ground.
        let source = "The bridge opened to traffic in 1998.";
        let context_only_quote = "She joined the club in 1985."; // absent from the source
        let votes = vec![model_verdict(Verdict::Supported, Some(context_only_quote))];
        let finding = assemble_citation_finding(source, &provenance(), &votes, 0);
        assert_eq!(finding.grounding_status, GroundingStatus::Unlocated);
        assert!(finding.passage.is_none());
    }

    #[test]
    fn end_to_end_model_failures_remain_panel_votes() {
        let fetch = StubHttpClient::new([Ok(HttpResponse {
            status: 200,
            headers: BTreeMap::from([("content-type".to_string(), "text/html".to_string())]),
            body: long_html_with("the bridge opened in 1998"),
        })]);
        let model_client = StubModelClient::new([
            Ok(completion(
                r#"{"verdict": "SUPPORTED", "quote": "the bridge opened in 1998"}"#,
            )),
            Err(model_transport_failure()),
            Err(model_transport_failure()),
        ]);
        let options = VerifyOptions {
            concurrency: 1,
            ..VerifyOptions::default()
        };
        let outcome = block_on(verify_citation_use_site(
            &fetch,
            &model_client,
            &FixedClock::new(1000),
            &[model(), model(), model()],
            &request("The bridge opened in 1998", "https://example.com/bridge"),
            None,
            3,
            options,
        ))
        .expect("verifies");

        assert_eq!(outcome.finding.verdict, CitationVerdict::SourceUnavailable);
        assert_eq!(outcome.finding.agreement.panel_size, 3);
        assert_eq!(outcome.finding.agreement.winner_votes, 2);
        assert_eq!(outcome.votes.len(), 3);
        assert_eq!(
            outcome.votes[0].verdict,
            CitationVerdict::Judged(SupportLevel::Supported)
        );
        assert_eq!(outcome.votes[1].verdict, CitationVerdict::SourceUnavailable);
        assert_eq!(outcome.votes[2].verdict, CitationVerdict::SourceUnavailable);
        assert!(
            outcome
                .votes
                .iter()
                .all(|vote| vote.invocation.prompt_hash.len() == 64)
        );
    }

    #[test]
    fn end_to_end_all_model_failures_surface_source_unavailable() {
        let fetch = StubHttpClient::new([Ok(HttpResponse {
            status: 200,
            headers: BTreeMap::from([("content-type".to_string(), "text/html".to_string())]),
            body: long_html_with("the museum was founded in 1850"),
        })]);
        let model_client = StubModelClient::new([
            Err(model_transport_failure()),
            Err(model_transport_failure()),
        ]);
        let options = VerifyOptions {
            concurrency: 1,
            ..VerifyOptions::default()
        };
        let outcome = block_on(verify_citation_use_site(
            &fetch,
            &model_client,
            &FixedClock::new(1000),
            &[model(), model()],
            &request("The museum opened in 1850", "https://example.com/museum"),
            None,
            0,
            options,
        ))
        .expect("verifies");

        assert_eq!(outcome.finding.verdict, CitationVerdict::SourceUnavailable);
        assert_eq!(outcome.finding.agreement.panel_size, 2);
        assert_eq!(outcome.finding.agreement.winner_votes, 2);
        assert_eq!(outcome.votes.len(), 2);
        assert!(
            outcome
                .votes
                .iter()
                .all(|vote| vote.verdict == CitationVerdict::SourceUnavailable)
        );
    }

    #[test]
    fn end_to_end_unreachable_source_is_source_unavailable_with_no_model_call() {
        // Only a failing fetch is queued; the model stub is empty — a model call would error.
        let fetch = StubHttpClient::new([Ok(HttpResponse {
            status: 404,
            headers: BTreeMap::new(),
            body: Vec::new(),
        })]);
        let model_client = StubModelClient::new([]);
        let outcome = block_on(verify_citation_use_site(
            &fetch,
            &model_client,
            &FixedClock::new(1),
            &[model()],
            &request("some claim", "https://example.com/missing"),
            None,
            0,
            VerifyOptions::default(),
        ))
        .expect("verifies");
        assert_eq!(outcome.finding.verdict, CitationVerdict::SourceUnavailable);
        assert!(outcome.finding.passage.is_none());
        assert!(outcome.votes.is_empty());
        // The failing HTTP status is preserved, so a retry path can tell a permanent
        // 404 from a retryable 429/503 — it is not discarded into the verdict.
        assert_eq!(outcome.finding.provenance.http_status, Some(404));
    }

    #[test]
    fn end_to_end_records_the_http_status_of_a_successful_fetch() {
        let fetch = StubHttpClient::new([Ok(HttpResponse {
            status: 200,
            headers: BTreeMap::from([("content-type".to_string(), "text/html".to_string())]),
            body: long_html_with("the museum was founded in 1850"),
        })]);
        let model_client = StubModelClient::new([Ok(completion(
            r#"{"verdict": "SUPPORTED", "quote": "the museum was founded in 1850"}"#,
        ))]);
        let outcome = block_on(verify_citation_use_site(
            &fetch,
            &model_client,
            &FixedClock::new(1),
            &[model()],
            &request(
                "The museum was founded in 1850",
                "https://example.com/museum",
            ),
            None,
            0,
            VerifyOptions::default(),
        ))
        .expect("verifies");
        assert_eq!(outcome.finding.provenance.http_status, Some(200));
    }

    #[test]
    fn end_to_end_fabricated_quote_is_unverified_not_groundable() {
        // Layer 6: the model claims SUPPORTED with a quote nowhere in the body. The verdict
        // is surfaced honestly (the panel judged it), but marked Unlocated and NOT groundable
        // — anti-fabrication is the action gate's job, not a silent downgrade.
        let fetch = StubHttpClient::new([Ok(HttpResponse {
            status: 200,
            headers: BTreeMap::from([("content-type".to_string(), "text/html".to_string())]),
            body: long_html_with("the museum was founded in 1850"),
        })]);
        let model_client = StubModelClient::new([Ok(completion(
            r#"{"verdict": "SUPPORTED", "quote": "a quote that is nowhere in the body"}"#,
        ))]);
        let outcome = block_on(verify_citation_use_site(
            &fetch,
            &model_client,
            &FixedClock::new(1),
            &[model()],
            &request("The museum opened in 1850", "https://example.com/museum"),
            None,
            0,
            VerifyOptions::default(),
        ))
        .expect("verifies");
        assert_eq!(outcome.finding.grounding_status, GroundingStatus::Unlocated);
        assert!(!is_groundable_support(&outcome.finding));
        assert!(outcome.finding.passage.is_none());
    }

    // --- guarded fuzzy locate at the gate (SP42#25 layer 5) ---

    #[test]
    fn assemble_fuzzy_grounds_a_near_miss_as_located_fuzzy_and_not_groundable() {
        let source = "In 1985 the Acme Corporation was established in Springfield by a group \
                      of local investors led by John Smith.";
        // One reworded token: exact locate fails, the guarded fuzzy path recovers. The
        // surfaced passage is the SOURCE's text; the finding is marked LocatedFuzzy and is
        // NOT groundable — the hard exact-locate gate guards any autonomous path.
        let finding = assemble_citation_finding(
            source,
            &provenance(),
            &[model_verdict(
                Verdict::Supported,
                Some(
                    "the Acme Corporation was founded in Springfield by a group of local investors",
                ),
            )],
            0,
        );
        assert_eq!(
            finding.verdict,
            CitationVerdict::Judged(SupportLevel::Supported)
        );
        assert_eq!(finding.grounding_status, GroundingStatus::LocatedFuzzy);
        assert!(!is_groundable_support(&finding));
        let passage = finding.passage.as_ref().expect("fuzzy passage");
        assert!(passage.quote.contains("established in Springfield"));
        assert!(!passage.quote.contains("founded"));
    }

    #[test]
    fn assemble_prefers_exact_locate_over_fuzzy() {
        let source = "In 1985 the Acme Corporation was established in Springfield by a group \
                      of local investors led by John Smith.";
        let finding = assemble_citation_finding(
            source,
            &provenance(),
            &[model_verdict(
                Verdict::Supported,
                Some("established in Springfield by a group of local investors"),
            )],
            0,
        );
        assert_eq!(finding.grounding_status, GroundingStatus::Located);
        assert!(is_groundable_support(&finding));
    }

    // --- repair turn (SP42#25 layer 3) ---

    #[test]
    fn repair_edge_parses_a_returned_span_and_fingerprints() {
        let client = StubModelClient::new([Ok(completion(r#"{"quote": "established in 1985"}"#))]);
        let attempt = block_on(execute_citation_repair(
            &client,
            &model(),
            &params(),
            inputs(
                "the company was founded in 1985",
                "Acme Corp was established in 1985.",
            ),
            "the company was founded in 1985",
        ))
        .expect("repairs");
        assert_eq!(attempt.quote.as_deref(), Some("established in 1985"));
        assert_eq!(attempt.invocation.prompt_hash.len(), 64);
        assert_eq!(attempt.invocation.model, model());
    }

    #[test]
    fn repair_edge_no_span_yields_no_quote() {
        let client = StubModelClient::new([Ok(completion(r#"{"quote": "NO_SPAN"}"#))]);
        let attempt = block_on(execute_citation_repair(
            &client,
            &model(),
            &params(),
            inputs("a claim", "a usable source body"),
            "a quote that failed",
        ))
        .expect("repairs");
        assert_eq!(attempt.quote, None);
    }

    #[test]
    fn assemble_grounds_a_support_via_the_repaired_quote() {
        // Layer 3: the original quote does not locate, the repaired one does — the finding
        // is Located on the repaired passage; the verdict was never up for re-litigation.
        let source = "Acme Corp was established in 1985 by its founder John Smith.";
        let finding = assemble_citation_finding(
            source,
            &provenance(),
            &[repaired_verdict(
                Verdict::Supported,
                Some("Acme was founded in 1985"),
                Some("established in 1985"),
            )],
            0,
        );
        assert_eq!(
            finding.verdict,
            CitationVerdict::Judged(SupportLevel::Supported)
        );
        assert_eq!(finding.grounding_status, GroundingStatus::Located);
        assert!(is_groundable_support(&finding));
        assert_eq!(
            finding.passage.as_ref().map(|p| p.quote.as_str()),
            Some("established in 1985")
        );
    }

    #[test]
    fn assemble_ignores_a_repair_that_still_does_not_locate() {
        let source = "Acme Corp was established in 1985.";
        let finding = assemble_citation_finding(
            source,
            &provenance(),
            &[repaired_verdict(
                Verdict::Supported,
                Some("founded in 1772"),
                Some("an invented repair span"),
            )],
            0,
        );
        assert_eq!(finding.grounding_status, GroundingStatus::Unlocated);
        assert!(!is_groundable_support(&finding));
        assert!(finding.passage.is_none());
    }

    #[test]
    fn build_model_votes_records_the_repair_audit_trail() {
        let source = "Acme Corp was established in 1985.";
        let votes = build_model_votes(
            &[repaired_verdict(
                Verdict::Supported,
                Some("Acme was founded in 1985"),
                Some("established in 1985"),
            )],
            source,
        );
        // The raw claimed quote stays the ORIGINAL (the repair never rewrites history);
        // the repaired span and its invocation fingerprint are recorded alongside.
        assert_eq!(
            votes[0].claimed_quote.as_deref(),
            Some("Acme was founded in 1985")
        );
        assert_eq!(
            votes[0].repaired_quote.as_deref(),
            Some("established in 1985")
        );
        assert_eq!(
            votes[0]
                .repair_invocation
                .as_ref()
                .map(|i| i.prompt_hash.as_str()),
            Some("repair")
        );
        // The located passage comes from the repaired span.
        assert_eq!(
            votes[0].located_passage.as_ref().map(|p| p.quote.as_str()),
            Some("established in 1985")
        );
    }

    #[test]
    fn end_to_end_repair_turn_recovers_a_transcription_miss() {
        let fetch = StubHttpClient::new([Ok(HttpResponse {
            status: 200,
            headers: BTreeMap::from([("content-type".to_string(), "text/html".to_string())]),
            body: long_html_with("the bridge opened to traffic in 1998"),
        })]);
        // Turn 1: SUPPORTED with a paraphrased (non-locating) quote. Turn 2 (repair): the
        // exact span. Stub is FIFO; panel of 1 at concurrency 1 keeps the order deterministic.
        let model_client = StubModelClient::new([
            Ok(completion(
                r#"{"verdict": "SUPPORTED", "quote": "the bridge was opened in 1998"}"#,
            )),
            Ok(completion(
                r#"{"quote": "the bridge opened to traffic in 1998"}"#,
            )),
        ]);
        let outcome = block_on(verify_citation_use_site(
            &fetch,
            &model_client,
            &FixedClock::new(1),
            &[model()],
            &request("The bridge opened in 1998", "https://example.com/bridge"),
            None,
            0,
            VerifyOptions::default(),
        ))
        .expect("verifies");
        assert_eq!(
            outcome.finding.verdict,
            CitationVerdict::Judged(SupportLevel::Supported)
        );
        assert_eq!(outcome.finding.grounding_status, GroundingStatus::Located);
        assert!(is_groundable_support(&outcome.finding));
        assert_eq!(
            outcome.votes[0].claimed_quote.as_deref(),
            Some("the bridge was opened in 1998")
        );
        assert_eq!(
            outcome.votes[0].repaired_quote.as_deref(),
            Some("the bridge opened to traffic in 1998")
        );
        assert!(outcome.votes[0].repair_invocation.is_some());
    }

    #[test]
    fn end_to_end_repair_disabled_makes_no_extra_model_call() {
        let fetch = StubHttpClient::new([Ok(HttpResponse {
            status: 200,
            headers: BTreeMap::from([("content-type".to_string(), "text/html".to_string())]),
            body: long_html_with("the bridge opened to traffic in 1998"),
        })]);
        // A locating repair response IS queued — if the repair ran, the finding would be
        // Located. With repair_turn off it must stay Unlocated (the response unconsumed).
        let model_client = StubModelClient::new([
            Ok(completion(
                r#"{"verdict": "SUPPORTED", "quote": "the bridge was opened in 1998"}"#,
            )),
            Ok(completion(
                r#"{"quote": "the bridge opened to traffic in 1998"}"#,
            )),
        ]);
        let options = VerifyOptions {
            repair_turn: false,
            ..VerifyOptions::default()
        };
        let outcome = block_on(verify_citation_use_site(
            &fetch,
            &model_client,
            &FixedClock::new(1),
            &[model()],
            &request("The bridge opened in 1998", "https://example.com/bridge"),
            None,
            0,
            options,
        ))
        .expect("verifies");
        assert_eq!(outcome.finding.grounding_status, GroundingStatus::Unlocated);
        assert!(outcome.votes[0].repair_invocation.is_none());
    }

    #[test]
    fn end_to_end_no_span_repair_stays_unlocated() {
        let fetch = StubHttpClient::new([Ok(HttpResponse {
            status: 200,
            headers: BTreeMap::from([("content-type".to_string(), "text/html".to_string())]),
            body: long_html_with("the museum was founded in 1850"),
        })]);
        let model_client = StubModelClient::new([
            Ok(completion(
                r#"{"verdict": "SUPPORTED", "quote": "a quote that is nowhere in the body"}"#,
            )),
            Ok(completion(r#"{"quote": "NO_SPAN"}"#)),
        ]);
        let outcome = block_on(verify_citation_use_site(
            &fetch,
            &model_client,
            &FixedClock::new(1),
            &[model()],
            &request("The museum opened in 1850", "https://example.com/museum"),
            None,
            0,
            VerifyOptions::default(),
        ))
        .expect("verifies");
        assert_eq!(outcome.finding.grounding_status, GroundingStatus::Unlocated);
        assert!(!is_groundable_support(&outcome.finding));
        // The repair was attempted (audit trail) but returned NO_SPAN.
        assert!(outcome.votes[0].repair_invocation.is_some());
        assert_eq!(outcome.votes[0].repaired_quote, None);
    }

    #[test]
    fn end_to_end_located_quote_triggers_no_repair() {
        let fetch = StubHttpClient::new([Ok(HttpResponse {
            status: 200,
            headers: BTreeMap::from([("content-type".to_string(), "text/html".to_string())]),
            body: long_html_with("the bridge opened in 1998"),
        })]);
        // Exactly ONE response queued: a repair attempt would consume a second and record
        // an (errored) attempt; a locating first quote must skip the repair entirely.
        let model_client = StubModelClient::new([Ok(completion(
            r#"{"verdict": "SUPPORTED", "quote": "the bridge opened in 1998"}"#,
        ))]);
        let outcome = block_on(verify_citation_use_site(
            &fetch,
            &model_client,
            &FixedClock::new(1),
            &[model()],
            &request("The bridge opened in 1998", "https://example.com/bridge"),
            None,
            0,
            VerifyOptions::default(),
        ))
        .expect("verifies");
        assert_eq!(outcome.finding.grounding_status, GroundingStatus::Located);
        assert!(outcome.votes[0].repair_invocation.is_none());
    }

    // --- source unavailable reason derivation ---

    #[test]
    fn su_reason_unusable_when_fetched_200() {
        // A SourceUnavailable finding whose provenance http_status == Some(200)
        // should have reason == Some(Unusable).
        let source = "This is a PDF or JavaScript viewer shell.";
        let mut provenance = provenance();
        provenance.http_status = Some(200);
        let finding = assemble_citation_finding(
            source,
            &provenance,
            &[model_verdict(Verdict::SourceUnavailable, None)],
            0,
        );
        assert_eq!(finding.verdict, CitationVerdict::SourceUnavailable);
        assert_eq!(
            finding.source_unavailable_reason,
            Some(super::SourceUnavailableReason::Unusable)
        );
    }

    #[test]
    fn su_reason_unreachable_when_fetch_failed() {
        // A SourceUnavailable finding whose provenance http_status == Some(404)
        // should have reason == Some(Unreachable).
        let source = "";
        let mut provenance = provenance();
        provenance.http_status = Some(404);
        let finding = assemble_citation_finding(
            source,
            &provenance,
            &[model_verdict(Verdict::SourceUnavailable, None)],
            0,
        );
        assert_eq!(finding.verdict, CitationVerdict::SourceUnavailable);
        assert_eq!(
            finding.source_unavailable_reason,
            Some(super::SourceUnavailableReason::Unreachable)
        );
    }

    #[test]
    fn su_reason_unreachable_when_http_status_none() {
        // A SourceUnavailable finding whose provenance http_status == None
        // (legacy snapshot) should default to Unreachable.
        let source = "";
        let mut provenance = provenance();
        provenance.http_status = None;
        let finding = assemble_citation_finding(
            source,
            &provenance,
            &[model_verdict(Verdict::SourceUnavailable, None)],
            0,
        );
        assert_eq!(finding.verdict, CitationVerdict::SourceUnavailable);
        assert_eq!(
            finding.source_unavailable_reason,
            Some(super::SourceUnavailableReason::Unreachable)
        );
    }

    #[test]
    fn su_reason_none_for_supported() {
        // A Supported finding should have source_unavailable_reason == None.
        let source = "Acme Corp was established in 1985.";
        let finding = assemble_citation_finding(
            source,
            &provenance(),
            &[model_verdict(
                Verdict::Supported,
                Some("established in 1985"),
            )],
            0,
        );
        assert_eq!(
            finding.verdict,
            CitationVerdict::Judged(SupportLevel::Supported)
        );
        assert_eq!(finding.source_unavailable_reason, None);
    }

    #[test]
    fn su_reason_none_for_not_supported() {
        // A NotSupported finding should have source_unavailable_reason == None.
        let source = "Acme Corp was established in 1985.";
        let finding = assemble_citation_finding(
            source,
            &provenance(),
            &[model_verdict(Verdict::NotSupported, None)],
            0,
        );
        assert_eq!(
            finding.verdict,
            CitationVerdict::Judged(SupportLevel::NotSupported)
        );
        assert_eq!(finding.source_unavailable_reason, None);
    }

    #[test]
    fn serde_back_compat_finding_without_reason_deserializes_to_none() {
        // ADR-0009 replay: a finding JSON written before `source_unavailable_reason`
        // existed must still load, with the field defaulting to None.
        let legacy = r#"{
            "kind":"citation_verdict",
            "verdict":"supported",
            "grounding_status":"located",
            "agreement":{"winner_votes":2,"panel_size":2},
            "passage":{"quote":"example","offset":0},
            "provenance":{"url":"https://example.com/","content_hash":"abc","fetched_at":42,"http_status":200},
            "grounding":{"kind":"located_quote","quote":"example","source_hash":"abc","offset":0},
            "use_site_ordinal":0,
            "schema_version":1
        }"#;
        let finding: CitationFinding =
            serde_json::from_str(legacy).expect("legacy finding deserializes");
        assert_eq!(finding.source_unavailable_reason, None);
        assert_eq!(
            finding.verdict,
            CitationVerdict::Judged(SupportLevel::Supported)
        );
    }

    #[test]
    fn serde_finding_with_reason_serializes_correctly() {
        // When a finding has source_unavailable_reason, it should serialize it.
        let source = "";
        let mut provenance = provenance();
        provenance.http_status = Some(404);
        let finding = assemble_citation_finding(
            source,
            &provenance,
            &[model_verdict(Verdict::SourceUnavailable, None)],
            0,
        );
        let json_str = serde_json::to_string(&finding).expect("serialize");
        assert!(json_str.contains("unreachable"));

        // Deserialize back and verify.
        let deserialized: CitationFinding = serde_json::from_str(&json_str).expect("deserialize");
        assert_eq!(
            deserialized.source_unavailable_reason,
            Some(super::SourceUnavailableReason::Unreachable)
        );
    }

    #[test]
    fn serde_finding_without_reason_omits_field_when_none() {
        // When source_unavailable_reason is None (for a Supported finding),
        // skip_serializing_if should omit it from the JSON.
        let source = "Acme Corp was established in 1985.";
        let finding = assemble_citation_finding(
            source,
            &provenance(),
            &[model_verdict(
                Verdict::Supported,
                Some("established in 1985"),
            )],
            0,
        );
        let json_str = serde_json::to_string(&finding).expect("serialize");
        assert!(!json_str.contains("source_unavailable_reason"));
    }

    proptest! {
        /// THE anti-fabrication guarantee (layer 6): a `Supported` vote whose quote cannot
        /// locate in the source is never GROUNDABLE — the verdict may be surfaced for a human,
        /// but `is_groundable_support` (the only autonomous-action gate) is always false.
        #[test]
        fn fabricated_support_is_never_groundable(
            source in "[a-m ]{0,200}",
            quote in "[n-z]{3,40}",
        ) {
            // quote uses only n-z; source only a-m + space => quote can never be a substring.
            let finding = assemble_citation_finding(
                &source,
                &provenance(),
                &[model_verdict(Verdict::Supported, Some(&quote))],
                0,
            );
            prop_assert!(!is_groundable_support(&finding));
            prop_assert_ne!(finding.grounding_status, GroundingStatus::Located);
        }

        /// Layer 5 must not weaken the guarantee either: a fabricated MULTI-TOKEN quote
        /// (eligible for the fuzzy path) over a disjoint-alphabet source is never Located
        /// OR LocatedFuzzy, and never groundable.
        #[test]
        fn fabricated_multi_token_quote_never_grounds_fuzzily(
            source in "[a-m ]{50,300}",
            quote in "[n-z]{4,9}( [n-z]{4,9}){5,12}",
        ) {
            let finding = assemble_citation_finding(
                &source,
                &provenance(),
                &[model_verdict(Verdict::Supported, Some(&quote))],
                0,
            );
            prop_assert!(!is_groundable_support(&finding));
            prop_assert_eq!(finding.grounding_status, GroundingStatus::Unlocated);
        }

        /// Layer 3 must not weaken the guarantee: a repair span that does not locate in
        /// the fetched source never grounds either — the repair turn gives the model a
        /// second chance at TRANSCRIPTION, never a bypass around the locate gate.
        #[test]
        fn fabricated_repair_is_never_groundable(
            source in "[a-m ]{0,200}",
            quote in "[n-z]{3,40}",
            repair in "[n-z]{3,40}",
        ) {
            let finding = assemble_citation_finding(
                &source,
                &provenance(),
                &[repaired_verdict(Verdict::Supported, Some(&quote), Some(&repair))],
                0,
            );
            prop_assert!(!is_groundable_support(&finding));
            prop_assert_ne!(finding.grounding_status, GroundingStatus::Located);
        }
    }

    #[test]
    fn short_body_records_unusable_reason_and_skips_panel() {
        let fetch = StubHttpClient::new([Ok(HttpResponse {
            status: 200,
            headers: BTreeMap::from([("content-type".to_string(), "text/html".to_string())]),
            body: b"<html><body>tiny</body></html>".to_vec(),
        })]);
        let model_client = StubModelClient::new([]); // empty → any model call errors
        let outcome = block_on(verify_citation_use_site(
            &fetch,
            &model_client,
            &FixedClock::new(1000),
            &[model()],
            &request("Some claim", "https://example.com/tiny"),
            None,
            3,
            VerifyOptions::default(),
        ))
        .expect("verifies");
        assert_eq!(outcome.finding.verdict, CitationVerdict::SourceUnavailable);
        assert_eq!(
            outcome.finding.unusable_reason,
            Some(BodyUsabilityReason::ShortBody)
        );
        assert!(outcome.votes.is_empty());
    }

    #[test]
    fn unreachable_source_has_no_unusable_reason() {
        // A non-2xx fetch is Unreachable, not fetched-but-unusable: the verdict is
        // SourceUnavailable with source_unavailable_reason == Unreachable, but
        // unusable_reason must stay None (it is only for fetched 2xx bodies). The
        // empty body would otherwise classify as ShortBody and mis-tag the reason.
        let fetch = StubHttpClient::new([Ok(HttpResponse {
            status: 404,
            headers: BTreeMap::new(),
            body: Vec::new(),
        })]);
        let model_client = StubModelClient::new([]); // empty → any model call errors
        let outcome = block_on(verify_citation_use_site(
            &fetch,
            &model_client,
            &FixedClock::new(1000),
            &[model()],
            &request("Some claim", "https://example.com/missing"),
            None,
            3,
            VerifyOptions::default(),
        ))
        .expect("verifies");
        assert_eq!(outcome.finding.verdict, CitationVerdict::SourceUnavailable);
        assert_eq!(
            outcome.finding.source_unavailable_reason,
            Some(super::SourceUnavailableReason::Unreachable)
        );
        assert_eq!(outcome.finding.unusable_reason, None);
        assert!(outcome.votes.is_empty());
    }

    #[test]
    fn pdf_source_is_unusable_with_no_model_call() {
        let fetch = StubHttpClient::new([Ok(HttpResponse {
            status: 200,
            headers: BTreeMap::from([("content-type".to_string(), "application/pdf".to_string())]),
            body: b"%PDF-1.7\n...binary report body...".to_vec(),
        })]);
        let model_client = StubModelClient::new([]); // empty → any model call errors
        let outcome = block_on(verify_citation_use_site(
            &fetch,
            &model_client,
            &FixedClock::new(1000),
            &[model()],
            &request("Claim from a PDF", "https://example.com/report.pdf"),
            None,
            3,
            VerifyOptions::default(),
        ))
        .expect("verifies");
        assert_eq!(outcome.finding.verdict, CitationVerdict::SourceUnavailable);
        assert_eq!(
            outcome.finding.unusable_reason,
            Some(BodyUsabilityReason::PdfBody)
        );
        assert!(outcome.votes.is_empty());
    }

    #[test]
    fn google_books_source_is_unusable_with_no_model_call() {
        let fetch = StubHttpClient::new([Ok(HttpResponse {
            status: 200,
            headers: BTreeMap::from([("content-type".to_string(), "text/html".to_string())]),
            body: long_html_with("Pernod Ricard"), // chrome with the entity name present
        })]);
        let model_client = StubModelClient::new([]);
        let outcome = block_on(verify_citation_use_site(
            &fetch,
            &model_client,
            &FixedClock::new(1000),
            &[model()],
            &request(
                "Some claim about a book",
                "https://books.google.com/books?id=abc",
            ),
            None,
            3,
            VerifyOptions::default(),
        ))
        .expect("verifies");
        assert_eq!(outcome.finding.verdict, CitationVerdict::SourceUnavailable);
        assert_eq!(
            outcome.finding.unusable_reason,
            Some(BodyUsabilityReason::ViewerShell)
        );
        assert!(outcome.votes.is_empty());
    }

    #[test]
    fn book_snippet_bypasses_only_the_short_body_floor() {
        // ADR-0018 Decision 4: a search-inside snippet body well below the
        // 300-char web-page floor still reaches the panel...
        let snippet = "Matilda longed for her parents to be good and loving.";
        assert!(snippet.chars().count() < 300, "test premise: short body");
        let fetch = StubHttpClient::new([]); // prefetched → any fetch errors the test
        let model_client = StubModelClient::new([Ok(sp42_types::ModelCompletion {
            text: r#"{"verdict":"SUPPORTED","quote":"good and loving"}"#.to_string(),
            served_model: None,
        })]);
        let options = VerifyOptions {
            repair_turn: false,
            prefetched: Some(super::FetchedSource {
                text: snippet.to_string(),
                status: 200,
                content_type: "text/plain".to_string(),
                raw_html: None,
                book_snippet: true,
            }),
            ..VerifyOptions::default()
        };
        let outcome = block_on(verify_citation_use_site(
            &fetch,
            &model_client,
            &FixedClock::new(1000),
            &[model()],
            &request(
                "Matilda wished for loving parents",
                "https://archive.org/details/matilda00dahl/page/42",
            ),
            None,
            0,
            options,
        ))
        .expect("verifies");
        assert_eq!(
            outcome.finding.verdict,
            CitationVerdict::Judged(SupportLevel::Supported)
        );
        assert_eq!(outcome.finding.grounding_status, GroundingStatus::Located);

        // ...while the same short body WITHOUT the snippet provenance still
        // short-circuits as ShortBody with no model call.
        let fetch = StubHttpClient::new([]);
        let model_client = StubModelClient::new([]);
        let options = VerifyOptions {
            repair_turn: false,
            prefetched: Some(super::FetchedSource {
                text: snippet.to_string(),
                status: 200,
                content_type: "text/plain".to_string(),
                raw_html: None,
                book_snippet: false,
            }),
            ..VerifyOptions::default()
        };
        let outcome = block_on(verify_citation_use_site(
            &fetch,
            &model_client,
            &FixedClock::new(1000),
            &[model()],
            &request(
                "Matilda wished for loving parents",
                "https://example.org/short-page",
            ),
            None,
            0,
            options,
        ))
        .expect("verifies");
        assert_eq!(outcome.finding.verdict, CitationVerdict::SourceUnavailable);
        assert_eq!(
            outcome.finding.unusable_reason,
            Some(BodyUsabilityReason::ShortBody)
        );
        assert!(outcome.votes.is_empty());

        // The bypass is scoped to the floor: an EMPTY snippet stays unusable
        // even with the book provenance flag.
        let fetch = StubHttpClient::new([]);
        let model_client = StubModelClient::new([]);
        let options = VerifyOptions {
            repair_turn: false,
            prefetched: Some(super::FetchedSource {
                text: String::new(),
                status: 200,
                content_type: "text/plain".to_string(),
                raw_html: None,
                book_snippet: true,
            }),
            ..VerifyOptions::default()
        };
        let outcome = block_on(verify_citation_use_site(
            &fetch,
            &model_client,
            &FixedClock::new(1000),
            &[model()],
            &request(
                "Matilda wished for loving parents",
                "https://archive.org/details/matilda00dahl",
            ),
            None,
            0,
            options,
        ))
        .expect("verifies");
        assert_eq!(outcome.finding.verdict, CitationVerdict::SourceUnavailable);
        assert!(outcome.votes.is_empty());
    }

    #[test]
    fn law360_paywall_stub_short_circuits_no_partial() {
        // The #42 case: a large nav-chrome/paywall body with the claim's entity in a
        // sidebar. Must classify Unusable and never reach the panel (no confabulated partial).
        let body = format!(
            "Home News Sections Account {} Subscribe to read the full article. Sign in to continue.",
            "Companies Pernod Ricard SA Gosling's Brown-Forman ".repeat(25)
        )
        .into_bytes();
        let fetch = StubHttpClient::new([Ok(HttpResponse {
            status: 200,
            headers: BTreeMap::from([("content-type".to_string(), "text/html".to_string())]),
            body,
        })]);
        let model_client = StubModelClient::new([]); // empty → any model call errors the test
        let outcome = block_on(verify_citation_use_site(
            &fetch,
            &model_client,
            &FixedClock::new(1000),
            &[model()],
            &request(
                "Gosling's has litigated over the mark against Pernod Ricard",
                "https://www.law360.com/articles/735000/x",
            ),
            None,
            3,
            VerifyOptions::default(),
        ))
        .expect("verifies");
        assert_eq!(outcome.finding.verdict, CitationVerdict::SourceUnavailable);
        assert_eq!(
            outcome.finding.unusable_reason,
            Some(BodyUsabilityReason::NavChromePaywall)
        );
        assert!(
            outcome.votes.is_empty(),
            "paywall stub must not reach the panel"
        );
    }

    #[test]
    fn unusable_reason_round_trips_and_legacy_defaults_to_none() {
        let fetch = StubHttpClient::new([Ok(HttpResponse {
            status: 200,
            headers: BTreeMap::from([("content-type".to_string(), "application/pdf".to_string())]),
            body: b"%PDF-1.7 body bytes".to_vec(),
        })]);
        let model_client = StubModelClient::new([]);
        let finding = block_on(verify_citation_use_site(
            &fetch,
            &model_client,
            &FixedClock::new(1000),
            &[model()],
            &request("A claim", "https://example.com/x.pdf"),
            None,
            3,
            VerifyOptions::default(),
        ))
        .expect("verifies")
        .finding;
        assert_eq!(finding.unusable_reason, Some(BodyUsabilityReason::PdfBody));

        // Round-trip preserves the reason.
        let json = serde_json::to_string(&finding).expect("serialize");
        let back: CitationFinding = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.unusable_reason, Some(BodyUsabilityReason::PdfBody));

        // Legacy record (field absent) deserializes to None via #[serde(default)].
        let mut value: serde_json::Value = serde_json::from_str(&json).expect("to value");
        value
            .as_object_mut()
            .expect("object")
            .remove("unusable_reason");
        let legacy: CitationFinding = serde_json::from_value(value).expect("legacy deserialize");
        assert_eq!(legacy.unusable_reason, None);
    }
}
