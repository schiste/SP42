//! `verify_claim` — the panel verification verb (PRD-0010, Phase 3).
//!
//! Wraps `verify_citation_use_site`: fetch (or use caller-supplied content) → extract → model
//! panel → verbatim grounding, returning the governed [`VerifyResult`]. The four-value verdict
//! taxonomy and the anti-fabrication grounding gate (ADR-0007/0008) are surfaced unchanged: a
//! quote is carried on the result only when it re-located verbatim in the source, so a fabricated
//! quote is never surfaced as evidence (even if the panel's verdict is otherwise support-class).

use sp42_citation::{
    CitationFinding, CitationVerificationRequest, FetchedSource, VerifyOptions,
    verify_citation_use_site,
};
use sp42_platform::CitationVerificationError;
use sp42_types::{Clock, HttpClient, ModelClient, ModelRef};
use url::Url;

use crate::{Source, Verdict, VerifyResult};

/// Sentinel source URL recorded for `{ text }` inputs carrying no `retrieved_from` provenance.
/// Never fetched (the prefetched body short-circuits the fetch) and `.invalid` never resolves.
const PREFETCHED_SENTINEL: &str = "https://source.prefetched.invalid/";

/// Verify whether `source` supports `claim`, via the model panel with verbatim grounding.
///
/// `source` may be a URL (fetched through the hardened pipeline) or caller-supplied content
/// (used as-is, no fetch). The returned [`VerifyResult`] carries the governed verdict and, for a
/// support-class verdict whose quote re-located, that quote.
///
/// # Errors
///
/// Returns [`CitationVerificationError`] if the source URL is invalid, the panel is empty, or a
/// `{ url }` source cannot be fetched.
pub async fn verify_claim<C, M>(
    fetch_client: &C,
    model_client: &M,
    clock: &dyn Clock,
    panel: &[ModelRef],
    claim: &str,
    source: &Source,
) -> Result<VerifyResult, CitationVerificationError>
where
    C: HttpClient + ?Sized,
    M: ModelClient + ?Sized,
{
    let request = CitationVerificationRequest {
        wiki_id: String::new(),
        rev_id: 0,
        title: String::new(),
        claim: claim.to_owned(),
        source_url: source_url_for(source)?,
    };
    let mut options = VerifyOptions::default();
    if let Source::Text { text, .. } = source {
        options.prefetched = Some(FetchedSource {
            text: text.clone(),
            status: 200,
            // Caller-supplied text: no transport content-type, and no raw HTML for the
            // usability gate's structured-paywall check (grounding uses `text`).
            content_type: "text/plain".to_owned(),
            raw_html: None,
            book_snippet: false,
        });
    }
    let outcome = verify_citation_use_site(
        fetch_client,
        model_client,
        clock,
        panel,
        &request,
        None,
        0,
        options,
    )
    .await?;
    Ok(to_verify_result(outcome.finding))
}

/// The source URL recorded on the request: the given URL, the `{ text }` provenance, or the
/// never-fetched sentinel.
fn source_url_for(source: &Source) -> Result<Url, CitationVerificationError> {
    let raw = match source {
        Source::Url { url } => url.as_str(),
        Source::Text { retrieved_from, .. } => {
            retrieved_from.as_deref().unwrap_or(PREFETCHED_SENTINEL)
        }
    };
    raw.parse()
        .map_err(|_| CitationVerificationError::InvalidRequest {
            message: format!("invalid source url {raw:?}"),
        })
}

/// Map the engine's finding onto the agent-facing result. The quote rides only when it grounded
/// (`passage` is `Some`), so an unlocated/fabricated quote is never surfaced.
fn to_verify_result(finding: CitationFinding) -> VerifyResult {
    VerifyResult {
        verdict: Verdict::from(finding.verdict),
        quote: finding.passage.map(|passage| passage.quote),
        agreement: Some(finding.agreement),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use sp42_types::{
        FixedClock, HttpResponse, ModelCompletion, ModelRef, StubHttpClient, StubModelClient,
    };

    use super::verify_claim;
    use crate::{Source, Verdict};

    // Plain extracted-text body (≥300 chars) containing the quote verbatim — used directly as a
    // prefetched `{ text }` source.
    const BODY_TEXT: &str = "The Foo Bridge is a suspension bridge spanning the River Bar in the \
        city of Bazton. Construction began in 1994 and the bridge opened in 1998 after several \
        delays caused by funding shortfalls. At completion it was the longest single-span \
        crossing in the region, carrying four lanes of traffic and a separated pedestrian and \
        cycle path along its eastern edge.";

    fn model() -> ModelRef {
        ModelRef::new("test", "test-model", "test-model")
    }

    fn completion(text: &str) -> ModelCompletion {
        ModelCompletion {
            text: text.to_owned(),
            served_model: None,
        }
    }

    fn html_body() -> Vec<u8> {
        format!("<html><body><p>{BODY_TEXT}</p></body></html>").into_bytes()
    }

    #[tokio::test]
    async fn url_source_supported_with_grounded_quote() {
        let fetch = StubHttpClient::new([Ok(HttpResponse {
            status: 200,
            headers: BTreeMap::from([("content-type".to_owned(), "text/html".to_owned())]),
            body: html_body(),
        })]);
        let models = StubModelClient::new([Ok(completion(
            r#"{"verdict": "SUPPORTED", "quote": "the bridge opened in 1998"}"#,
        ))]);
        let result = verify_claim(
            &fetch,
            &models,
            &FixedClock::new(0),
            &[model()],
            "The bridge opened in 1998",
            &Source::Url {
                url: "https://example.com/bridge".to_owned(),
            },
        )
        .await
        .expect("verifies");
        assert_eq!(result.verdict, Verdict::Supported);
        assert_eq!(result.quote.as_deref(), Some("the bridge opened in 1998"));
        assert!(result.agreement.is_some());
    }

    #[tokio::test]
    async fn text_source_skips_fetch_with_same_grounding() {
        // Empty HTTP queue: if it tried to fetch, the stub would error.
        let fetch = StubHttpClient::new([]);
        let models = StubModelClient::new([Ok(completion(
            r#"{"verdict": "SUPPORTED", "quote": "the bridge opened in 1998"}"#,
        ))]);
        let result = verify_claim(
            &fetch,
            &models,
            &FixedClock::new(0),
            &[model()],
            "The bridge opened in 1998",
            &Source::Text {
                text: BODY_TEXT.to_owned(),
                retrieved_from: None,
            },
        )
        .await
        .expect("verifies");
        assert_eq!(result.verdict, Verdict::Supported);
        assert_eq!(result.quote.as_deref(), Some("the bridge opened in 1998"));
    }

    #[tokio::test]
    async fn fabricated_quote_is_never_surfaced() {
        // The model claims support with a quote absent from the body. The repair turn (on by
        // default) also yields NO_SPAN. The anti-fabrication guarantee: no quote is surfaced.
        let fetch = StubHttpClient::new([]);
        let models = StubModelClient::new([
            Ok(completion(
                r#"{"verdict": "SUPPORTED", "quote": "a span that is absent verbatim"}"#,
            )),
            Ok(completion("NO_SPAN")),
        ]);
        let result = verify_claim(
            &fetch,
            &models,
            &FixedClock::new(0),
            &[model()],
            "The bridge opened in 1998",
            &Source::Text {
                text: BODY_TEXT.to_owned(),
                retrieved_from: None,
            },
        )
        .await
        .expect("verifies");
        assert!(
            result.quote.is_none(),
            "a quote absent from the source must never be surfaced as evidence"
        );
    }
}
