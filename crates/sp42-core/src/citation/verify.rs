//! The citation-verification contract + edge + grounding gate (ADR-0008, ADR-0007 §5).
//!
//! Layering:
//! - **Contract types** ([`CitationVerificationRequest`], [`CitationFinding`], …) — the
//!   read-only Finding surface (ADR-0008 §1/§2). No numeric confidence field; a
//!   `CitationFinding` derives `Eq`.
//! - **Per-model edge** ([`build_citation_verify_request`] / [`execute_citation_verify`] /
//!   [`parse_citation_verify_response`]) — one model, one verdict, over the injected
//!   `HttpClient` (ADR-0008 §3). The parser ends in a validate gate that defaults an
//!   unrecoverable response to *not supported*, never to a support judgment.
//! - **Grounding gate** ([`assemble_citation_finding`]) — pure: votes the panel
//!   (ADR-0006), then independently re-locates the winning quote in the fetched bytes; a
//!   `Supported`/`Partial` whose quote does not locate is **suppressed** to not-supported
//!   pre-surface (ADR-0007 §5). The model is never trusted on its word.
//! - **Orchestration** ([`verify_citation_use_site`]) — async: fetch the source once,
//!   run the deterministic body-usability gate (short-circuit to `SourceUnavailable` with
//!   no model call), then fan the panel out with bounded concurrency and assemble.
//!
//! The per-model HTTP request needs the **fetched source body**, which
//! [`CitationVerificationRequest`] (claim + URL) does not carry; the edge therefore takes
//! a prepared [`VerifyModelInputs`] — see `docs/implementation-notes/ADR-CHANGE-NOTES.md`.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use url::Url;

use super::body_classifier::classify_body_usability;
use super::citoid::{
    CitoidMetadata, build_citoid_header, build_citoid_request, parse_citoid_response,
};
use super::concurrency::map_with_concurrency;
use super::locate_quote::locate_quote;
use super::parsing::{ParsedVerdict, parse_verdict_response};
use super::prompts::build_verify_prompt;
use super::source_fetch::{html_to_text, looks_like_html, recover_wayback_body};
use super::urls::rewrite_wayback_url;
use super::verdict::{CitationFindingKind, CitationVerdict, SupportLevel, Verdict};
use super::voting::{PanelAgreement, n_class_vote};
use crate::errors::CitationVerificationError;
use crate::traits::{Clock, HttpClient};
use crate::types::{HttpMethod, HttpRequest, WikiConfig};

/// The schema version stamped on a [`CitationFinding`] (ADR-0008 §6).
pub const SCHEMA_VERSION: u32 = 1;

/// Identity of a model that produced an output — provider, model, and pinned version
/// (ADR-0006 Decision 8). Never a key or token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRef {
    /// The provider (e.g. `openrouter`, `local`).
    pub provider: String,
    /// The model id sent in the request.
    pub model: String,
    /// The pinned model version recorded for reproducibility (often equal to `model`).
    pub version: String,
}

impl ModelRef {
    /// Construct a model reference.
    #[must_use]
    pub fn new(
        provider: impl Into<String>,
        model: impl Into<String>,
        version: impl Into<String>,
    ) -> Self {
        Self {
            provider: provider.into(),
            model: model.into(),
            version: version.into(),
        }
    }
}

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

/// The read-only verification result — a Finding, never an action (ADR-0008 §2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CitationFinding {
    /// The finding kind (single value today).
    pub kind: CitationFindingKind,
    /// The voted categorical verdict (ADR-0007).
    pub verdict: CitationVerdict,
    /// Measured agreement among the panel's votes (ADR-0006).
    pub agreement: PanelAgreement,
    /// The winning verdict's located passage, or `None`.
    #[serde(default)]
    pub passage: Option<LocatedPassage>,
    /// Provenance of the really-fetched source.
    pub provenance: SourceProvenance,
    /// The machine-checkable grounding assertion.
    pub grounding: GroundingAssertion,
    /// Document-order position of this use-site (ADR-0007 §2).
    #[serde(default)]
    pub use_site_ordinal: u32,
    /// Schema version (Art. 9.1).
    pub schema_version: u32,
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
}

/// Options for a verification run.
#[derive(Debug, Clone, Copy)]
pub struct VerifyOptions {
    /// Whether to fetch the Citoid metadata sidecar (best-effort).
    pub include_metadata: bool,
    /// Maximum concurrent model calls.
    pub concurrency: usize,
}

impl Default for VerifyOptions {
    fn default() -> Self {
        Self {
            include_metadata: false,
            concurrency: 3,
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

/// Build the per-model verification HTTP request (OpenAI-compatible chat completion).
///
/// # Errors
///
/// Returns [`CitationVerificationError::InvalidRequest`] if the claim/source text is
/// empty or `config.inference_url` is unset, or a serialization error.
pub fn build_citation_verify_request(
    config: &WikiConfig,
    model: &ModelRef,
    inputs: VerifyModelInputs<'_>,
) -> Result<HttpRequest, CitationVerificationError> {
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
    let url =
        config
            .inference_url
            .clone()
            .ok_or_else(|| CitationVerificationError::InvalidRequest {
                message: "inference_url is not configured".to_string(),
            })?;
    let messages = build_verify_prompt(
        inputs.claim,
        inputs.source_text,
        inputs.source_url,
        inputs.metadata,
    );
    let body = serde_json::to_vec(&serde_json::json!({
        "model": model.model,
        "messages": messages,
        "temperature": 0,
    }))?;
    Ok(HttpRequest {
        method: HttpMethod::Post,
        url,
        headers: BTreeMap::from([("content-type".to_string(), "application/json".to_string())]),
        body,
    })
}

/// Parse a model chat-completion response into a [`ParsedVerdict`].
///
/// An unrecoverable verdict in an otherwise-valid envelope defaults to *not supported*
/// (the validate gate), never a support judgment (ADR-0008 §3).
///
/// # Errors
///
/// Returns [`CitationVerificationError::InvalidResponse`] if the envelope has no
/// `choices[0].message.content`, or a JSON parse error.
pub fn parse_citation_verify_response(
    body: &[u8],
) -> Result<ParsedVerdict, CitationVerificationError> {
    let parsed: Value = serde_json::from_slice(body)?;
    let content = parsed
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .ok_or_else(|| CitationVerificationError::InvalidResponse {
            message: "response has no choices[0].message.content".to_string(),
        })?;
    Ok(parse_verdict_response(content).unwrap_or(ParsedVerdict {
        verdict: Verdict::NotSupported,
        quote: None,
    }))
}

/// Run one model's verification over the injected `HttpClient`.
///
/// # Errors
///
/// Returns [`CitationVerificationError`] on a build, transport, non-2xx, or parse error.
pub async fn execute_citation_verify<C>(
    client: &C,
    config: &WikiConfig,
    model: &ModelRef,
    inputs: VerifyModelInputs<'_>,
) -> Result<ParsedVerdict, CitationVerificationError>
where
    C: HttpClient + ?Sized,
{
    let request = build_citation_verify_request(config, model, inputs)?;
    let response = client.execute(request).await.map_err(|error| {
        CitationVerificationError::InvalidResponse {
            message: error.to_string(),
        }
    })?;
    if !(200..300).contains(&response.status) {
        return Err(CitationVerificationError::InvalidResponse {
            message: format!("unexpected HTTP status {}", response.status),
        });
    }
    parse_citation_verify_response(&response.body)
}

/// Assemble the final [`CitationFinding`] from the panel's parsed votes — the
/// anti-fabrication grounding gate (ADR-0007 §5).
///
/// Votes the panel, then for a `Supported`/`Partial` winner re-locates a winning-class
/// quote in `source_text`; if none locates, the support claim is suppressed to
/// not-supported. A no-quote winner grounds on "source fetched".
#[must_use]
pub fn assemble_citation_finding(
    source_text: &str,
    provenance: &SourceProvenance,
    votes: &[ParsedVerdict],
    use_site_ordinal: u32,
) -> CitationFinding {
    let verdicts: Vec<Verdict> = votes.iter().map(|vote| vote.verdict).collect();
    let Some(vote) = n_class_vote(&verdicts) else {
        return no_quote_finding(
            CitationVerdict::SourceUnavailable,
            PanelAgreement::new(0, 0),
            provenance,
            use_site_ordinal,
        );
    };

    if vote.winner.is_support_class() {
        let located = votes
            .iter()
            .filter(|candidate| candidate.verdict == vote.winner)
            .find_map(|candidate| {
                let quote = candidate.quote.as_ref()?;
                let offset = locate_quote(quote, source_text)?;
                Some((quote.clone(), offset))
            });

        return match located {
            Some((quote, offset)) => CitationFinding {
                kind: CitationFindingKind::CitationVerdict,
                verdict: CitationVerdict::from(vote.winner),
                agreement: vote.agreement,
                passage: Some(LocatedPassage {
                    quote: quote.clone(),
                    offset,
                }),
                provenance: provenance.clone(),
                grounding: GroundingAssertion::LocatedQuote {
                    quote,
                    source_hash: provenance.content_hash.clone(),
                    offset,
                },
                use_site_ordinal,
                schema_version: SCHEMA_VERSION,
            },
            // Anti-fabrication: a support claim whose quote does not locate is suppressed.
            None => no_quote_finding(
                CitationVerdict::Judged(SupportLevel::NotSupported),
                vote.agreement,
                provenance,
                use_site_ordinal,
            ),
        };
    }

    no_quote_finding(
        CitationVerdict::from(vote.winner),
        vote.agreement,
        provenance,
        use_site_ordinal,
    )
}

/// A finding with no located passage, grounded on "the source was fetched".
fn no_quote_finding(
    verdict: CitationVerdict,
    agreement: PanelAgreement,
    provenance: &SourceProvenance,
    use_site_ordinal: u32,
) -> CitationFinding {
    CitationFinding {
        kind: CitationFindingKind::CitationVerdict,
        verdict,
        agreement,
        passage: None,
        provenance: provenance.clone(),
        grounding: GroundingAssertion::SourceFetched {
            source_hash: provenance.content_hash.clone(),
        },
        use_site_ordinal,
        schema_version: SCHEMA_VERSION,
    }
}

/// A fetched source body plus the HTTP status it came from.
struct FetchedSource {
    text: String,
    #[allow(dead_code)]
    status: u16,
}

/// Fetch a source body (read-only GET), extracting text from HTML and recovering past a
/// surviving Wayback banner. A non-2xx/3xx yields empty text (→ `SourceUnavailable`).
async fn fetch_source<C>(
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
        });
    }
    let content_type = response
        .headers
        .get("content-type")
        .cloned()
        .unwrap_or_default();
    let body = String::from_utf8_lossy(&response.body).into_owned();
    let text = if looks_like_html(&content_type, &body) {
        html_to_text(&body)
    } else {
        body
    };
    Ok(FetchedSource {
        text: recover_wayback_body(&text),
        status: response.status,
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

/// Verify one (claim, source) use-site end-to-end (ADR-0008 §3, ADR-0007).
///
/// Fetches the source once, runs the deterministic body-usability gate (short-circuiting
/// to `SourceUnavailable` with **no model call**), then fans the panel out with bounded
/// concurrency and assembles the grounded finding. Performs only GET requests — no writes.
///
/// # Errors
///
/// Returns [`CitationVerificationError`] for an empty panel, an unfetchable source URL, or
/// when every panel model fails.
pub async fn verify_citation_use_site<C>(
    client: &C,
    clock: &dyn Clock,
    config: &WikiConfig,
    panel: &[ModelRef],
    request: &CitationVerificationRequest,
    use_site_ordinal: u32,
    options: VerifyOptions,
) -> Result<CitationFinding, CitationVerificationError>
where
    C: HttpClient + ?Sized,
{
    if panel.is_empty() {
        return Err(CitationVerificationError::InvalidRequest {
            message: "model panel is empty".to_string(),
        });
    }

    let fetched = fetch_source(client, request.source_url.as_str()).await?;
    let provenance = SourceProvenance {
        url: request.source_url.clone(),
        content_hash: sha256_hex(fetched.text.as_bytes()),
        fetched_at: clock.now_ms(),
    };

    let body = if fetched.text.is_empty() {
        None
    } else {
        Some(fetched.text.as_str())
    };
    if !classify_body_usability(body).usable {
        return Ok(no_quote_finding(
            CitationVerdict::SourceUnavailable,
            PanelAgreement::new(0, 0),
            &provenance,
            use_site_ordinal,
        ));
    }

    let metadata = if options.include_metadata {
        fetch_metadata(client, request.source_url.as_str()).await
    } else {
        None
    };
    let inputs = VerifyModelInputs {
        claim: &request.claim,
        source_text: &fetched.text,
        source_url: request.source_url.as_str(),
        metadata: metadata.as_ref(),
    };

    let concurrency = options.concurrency.max(1);
    let results = map_with_concurrency(panel.to_vec(), concurrency, |model, _index| async move {
        execute_citation_verify(client, config, &model, inputs).await
    })
    .await;
    let votes: Vec<ParsedVerdict> = results.into_iter().filter_map(Result::ok).collect();
    if votes.is_empty() {
        return Err(CitationVerificationError::InvalidResponse {
            message: "all panel models failed".to_string(),
        });
    }

    Ok(assemble_citation_finding(
        &fetched.text,
        &provenance,
        &votes,
        use_site_ordinal,
    ))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use futures::executor::block_on;
    use proptest::prelude::*;

    use super::{
        CitationFinding, CitationVerificationRequest, GroundingAssertion, ModelRef,
        SourceProvenance, VerifyModelInputs, VerifyOptions, assemble_citation_finding,
        build_citation_verify_request, execute_citation_verify, parse_citation_verify_response,
        verify_citation_use_site,
    };
    use crate::citation::parsing::ParsedVerdict;
    use crate::citation::verdict::{CitationVerdict, SupportLevel, Verdict};
    use crate::traits::{FixedClock, StubHttpClient};
    use crate::types::{HttpMethod, HttpResponse, WikiConfig};

    fn config_with_inference() -> WikiConfig {
        let mut config = crate::test_fixtures::fixture_wiki_config();
        config.inference_url = Some(
            "https://inference.example/v1/chat/completions"
                .parse()
                .expect("valid url"),
        );
        config
    }

    fn model() -> ModelRef {
        ModelRef::new("openrouter", "test-model", "test-model")
    }

    fn inputs<'a>(claim: &'a str, source: &'a str) -> VerifyModelInputs<'a> {
        VerifyModelInputs {
            claim,
            source_text: source,
            source_url: "https://example.com",
            metadata: None,
        }
    }

    fn openai_response(content: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "choices": [ { "message": { "content": content } } ]
        }))
        .expect("serialize")
    }

    fn provenance() -> SourceProvenance {
        SourceProvenance {
            url: "https://example.com".parse().expect("url"),
            content_hash: "deadbeef".to_string(),
            fetched_at: 42,
        }
    }

    fn vote(verdict: Verdict, quote: Option<&str>) -> ParsedVerdict {
        ParsedVerdict {
            verdict,
            quote: quote.map(ToString::to_string),
        }
    }

    #[test]
    fn build_request_targets_inference_url() {
        let config = config_with_inference();
        let request = build_citation_verify_request(&config, &model(), inputs("a claim", "a body"))
            .expect("builds");
        assert_eq!(request.method, HttpMethod::Post);
        assert_eq!(
            request.url.as_str(),
            "https://inference.example/v1/chat/completions"
        );
        let body = String::from_utf8(request.body).expect("utf8");
        assert!(body.contains("test-model"));
        assert!(body.contains("two-step process"));
    }

    #[test]
    fn build_request_requires_inference_url() {
        let config = crate::test_fixtures::fixture_wiki_config(); // no inference_url
        let error = build_citation_verify_request(&config, &model(), inputs("c", "b"))
            .expect_err("should fail");
        assert!(error.to_string().contains("inference_url"));
    }

    #[test]
    fn parse_extracts_verdict_from_openai_envelope() {
        let body = openai_response(r#"{"verdict": "SUPPORTED", "quote": "x"}"#);
        let parsed = parse_citation_verify_response(&body).expect("parses");
        assert_eq!(parsed.verdict, Verdict::Supported);
        assert_eq!(parsed.quote.as_deref(), Some("x"));
    }

    #[test]
    fn parse_defaults_unrecoverable_content_to_not_supported() {
        let body = openai_response("i could not tell you, honestly");
        let parsed = parse_citation_verify_response(&body).expect("parses");
        assert_eq!(parsed.verdict, Verdict::NotSupported);
        assert_eq!(parsed.quote, None);
    }

    #[test]
    fn parse_rejects_a_malformed_envelope() {
        assert!(parse_citation_verify_response(b"{}").is_err());
    }

    #[test]
    fn execute_runs_through_the_http_trait() {
        let config = config_with_inference();
        let client = StubHttpClient::new([Ok(HttpResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: openai_response(r#"{"verdict": "PARTIAL", "quote": "it is believed"}"#),
        })]);
        let parsed = block_on(execute_citation_verify(
            &client,
            &config,
            &model(),
            inputs(
                "the treaty was signed in Paris",
                "It is believed the treaty was signed in Paris.",
            ),
        ))
        .expect("executes");
        assert_eq!(parsed.verdict, Verdict::Partial);
    }

    #[test]
    fn assemble_grounds_a_supported_verdict_with_a_locatable_quote() {
        let source = "Acme Corp was established in 1985 by its founder John Smith.";
        let finding = assemble_citation_finding(
            source,
            &provenance(),
            &[vote(Verdict::Supported, Some("established in 1985"))],
            7,
        );
        assert_eq!(
            finding.verdict,
            CitationVerdict::Judged(SupportLevel::Supported)
        );
        assert_eq!(finding.use_site_ordinal, 7);
        assert!(matches!(
            finding.grounding,
            GroundingAssertion::LocatedQuote { .. }
        ));
        assert!(finding.passage.is_some());
    }

    #[test]
    fn assemble_suppresses_a_supported_verdict_whose_quote_is_absent() {
        // THE anti-fabrication gate: a fabricated quote that is not in the source.
        let source = "Acme Corp was established in 1985.";
        let finding = assemble_citation_finding(
            source,
            &provenance(),
            &[vote(
                Verdict::Supported,
                Some("founded in 1772 by Napoleon"),
            )],
            0,
        );
        assert_eq!(
            finding.verdict,
            CitationVerdict::Judged(SupportLevel::NotSupported)
        );
        assert!(finding.passage.is_none());
        assert!(matches!(
            finding.grounding,
            GroundingAssertion::SourceFetched { .. }
        ));
    }

    #[test]
    fn assemble_not_supported_grounds_on_source_fetched() {
        let finding = assemble_citation_finding(
            "a body",
            &provenance(),
            &[vote(Verdict::NotSupported, None)],
            0,
        );
        assert_eq!(
            finding.verdict,
            CitationVerdict::Judged(SupportLevel::NotSupported)
        );
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
                vote(Verdict::Supported, Some("established in 1985")),
                vote(Verdict::NotSupported, None),
            ],
            0,
        );
        // Tie Supported vs NotSupported -> NotSupported (never up to Supported).
        assert_eq!(
            finding.verdict,
            CitationVerdict::Judged(SupportLevel::NotSupported)
        );
    }

    fn long_html_with(quote: &str) -> Vec<u8> {
        let padding =
            "This is real article prose that gives the body enough length to be usable. ".repeat(8);
        format!("<html><body><p>{padding}{quote}. {padding}</p></body></html>").into_bytes()
    }

    #[test]
    fn end_to_end_supported_finding() {
        let config = config_with_inference();
        let clock = FixedClock::new(1000);
        let client = StubHttpClient::new([
            Ok(HttpResponse {
                status: 200,
                headers: BTreeMap::from([("content-type".to_string(), "text/html".to_string())]),
                body: long_html_with("the bridge opened in 1998"),
            }),
            Ok(HttpResponse {
                status: 200,
                headers: BTreeMap::new(),
                body: openai_response(
                    r#"{"verdict": "SUPPORTED", "quote": "the bridge opened in 1998"}"#,
                ),
            }),
        ]);
        let request = CitationVerificationRequest {
            wiki_id: "enwiki".to_string(),
            rev_id: 1,
            title: "Bridge".to_string(),
            claim: "The bridge opened in 1998".to_string(),
            source_url: "https://example.com/bridge".parse().expect("url"),
        };
        let finding: CitationFinding = block_on(verify_citation_use_site(
            &client,
            &clock,
            &config,
            &[model()],
            &request,
            3,
            VerifyOptions::default(),
        ))
        .expect("verifies");
        assert_eq!(
            finding.verdict,
            CitationVerdict::Judged(SupportLevel::Supported)
        );
        assert_eq!(finding.provenance.fetched_at, 1000);
        assert_eq!(finding.use_site_ordinal, 3);
    }

    #[test]
    fn end_to_end_unreachable_source_is_source_unavailable_with_no_model_call() {
        let config = config_with_inference();
        let clock = FixedClock::new(1);
        // Only the source-fetch response is queued; a model call would error (no response).
        let client = StubHttpClient::new([Ok(HttpResponse {
            status: 404,
            headers: BTreeMap::new(),
            body: Vec::new(),
        })]);
        let request = CitationVerificationRequest {
            wiki_id: "enwiki".to_string(),
            rev_id: 1,
            title: "X".to_string(),
            claim: "some claim".to_string(),
            source_url: "https://example.com/missing".parse().expect("url"),
        };
        let finding = block_on(verify_citation_use_site(
            &client,
            &clock,
            &config,
            &[model()],
            &request,
            0,
            VerifyOptions::default(),
        ))
        .expect("verifies");
        assert_eq!(finding.verdict, CitationVerdict::SourceUnavailable);
        assert!(finding.passage.is_none());
    }

    #[test]
    fn end_to_end_fabricated_quote_is_suppressed() {
        let config = config_with_inference();
        let clock = FixedClock::new(1);
        let client = StubHttpClient::new([
            Ok(HttpResponse {
                status: 200,
                headers: BTreeMap::from([("content-type".to_string(), "text/html".to_string())]),
                body: long_html_with("the museum was founded in 1850"),
            }),
            Ok(HttpResponse {
                status: 200,
                headers: BTreeMap::new(),
                body: openai_response(
                    r#"{"verdict": "SUPPORTED", "quote": "a quote that is nowhere in the body"}"#,
                ),
            }),
        ]);
        let request = CitationVerificationRequest {
            wiki_id: "enwiki".to_string(),
            rev_id: 1,
            title: "Museum".to_string(),
            claim: "The museum opened in 1850".to_string(),
            source_url: "https://example.com/museum".parse().expect("url"),
        };
        let finding = block_on(verify_citation_use_site(
            &client,
            &clock,
            &config,
            &[model()],
            &request,
            0,
            VerifyOptions::default(),
        ))
        .expect("verifies");
        assert_eq!(
            finding.verdict,
            CitationVerdict::Judged(SupportLevel::NotSupported)
        );
        assert!(finding.passage.is_none());
    }

    proptest! {
        #[test]
        fn supported_is_never_surfaced_without_a_locatable_quote(
            source in "[a-m ]{0,200}",
            quote in "[n-z]{3,40}",
        ) {
            // quote uses only n-z; source uses only a-m + space => quote can never be a substring.
            let finding = assemble_citation_finding(
                &source,
                &provenance(),
                &[vote(Verdict::Supported, Some(&quote))],
                0,
            );
            prop_assert_ne!(finding.verdict, CitationVerdict::Judged(SupportLevel::Supported));
        }
    }
}
