//! `probe_source` — deterministic, model-free source accessibility (PRD-0010, Phase 2).
//!
//! Wraps `sp42-core`'s hardened fetch (`fetch_source`) and the deterministic source-usability gate
//! (`classify_source_usability`), mapping their results into [`ProbeResult`]. No model is ever
//! called. The verb's value is distinguishing *unreachable* from *reachable-but-unextractable*,
//! so a consumer learns that a human could still read a source the pipeline cannot use.

use sp42_citation::{classify_source_usability, fetch_source};
use sp42_types::HttpClient;

use crate::ProbeResult;

/// Probe whether SP42's pipeline can fetch and extract a source URL — without any model call.
///
/// `reachable` is a 2xx fetch; `extractable` is a reachable body the usability gate accepts.
/// `human_readable_hint` is set when the source is reachable but not pipeline-extractable.
pub async fn probe_source<C>(client: &C, url: &str) -> ProbeResult
where
    C: HttpClient + ?Sized,
{
    let Ok(fetched) = fetch_source(client, url).await else {
        // Transport failure / unparseable URL: not reachable, nothing to extract.
        return ProbeResult {
            reachable: false,
            http_status: None,
            extractable: false,
            unusable_reason: None,
            human_readable_hint: false,
        };
    };

    if !(200..300).contains(&fetched.status) {
        // Reached the network but got a non-2xx (link rot / blocked); no usable body.
        return ProbeResult {
            reachable: false,
            http_status: Some(fetched.status),
            extractable: false,
            unusable_reason: None,
            human_readable_hint: false,
        };
    }

    // Use the SAME gate as verification (URL + content-type + raw HTML), not just the text-shape
    // subset. A weaker gate here would let a PDF or a viewer/embed shell probe as extractable while
    // `verify_claim` immediately returns SourceUnavailable — making the probe disagree with the
    // path it exists to preflight.
    let usability = classify_source_usability(
        url,
        &fetched.content_type,
        fetched.raw_html.as_deref(),
        (!fetched.text.is_empty()).then_some(fetched.text.as_str()),
    );
    let extractable = usability.usable;
    ProbeResult {
        reachable: true,
        http_status: Some(fetched.status),
        extractable,
        unusable_reason: if extractable {
            None
        } else {
            Some(usability.reason)
        },
        human_readable_hint: !extractable,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use sp42_types::{HttpClientError, HttpResponse, StubHttpClient};

    use super::probe_source;
    use crate::BodyUsabilityReason;

    fn response_with_content_type(status: u16, content_type: &str, body: &str) -> HttpResponse {
        let mut headers = BTreeMap::new();
        headers.insert("content-type".to_owned(), content_type.to_owned());
        HttpResponse {
            status,
            headers,
            body: body.as_bytes().to_vec(),
        }
    }

    fn response(status: u16, body: &str) -> HttpResponse {
        response_with_content_type(status, "text/plain; charset=utf-8", body)
    }

    // ~400 chars of trigger-free prose that clears the short-body floor and the usability gate.
    const CLEAN_BODY: &str = "The Foo Bridge is a suspension bridge spanning the River Bar in \
        the city of Bazton. Construction began in 1994 and the bridge opened to traffic in 1998 \
        after several delays caused by funding shortfalls. At the time of its completion it was \
        the longest single-span crossing in the region, carrying four lanes of traffic and a \
        separated pedestrian and cycle path along its eastern edge.";

    #[tokio::test]
    async fn reachable_clean_body_is_extractable() {
        let client = StubHttpClient::new([Ok(response(200, CLEAN_BODY))]);
        let result = probe_source(&client, "https://example.org/foo-bridge").await;
        assert!(result.reachable);
        assert_eq!(result.http_status, Some(200));
        assert!(result.extractable);
        assert_eq!(result.unusable_reason, None);
        assert!(!result.human_readable_hint);
    }

    #[tokio::test]
    async fn reachable_anti_bot_body_is_not_extractable_but_human_readable() {
        let body = format!("Please enable JavaScript and cookies to continue. {CLEAN_BODY}");
        let client = StubHttpClient::new([Ok(response(200, &body))]);
        let result = probe_source(&client, "https://example.org/blocked").await;
        assert!(result.reachable);
        assert!(!result.extractable);
        assert_eq!(
            result.unusable_reason,
            Some(BodyUsabilityReason::AntiBotChallenge)
        );
        // The whole point: a human could still read this even though the model cannot.
        assert!(result.human_readable_hint);
    }

    #[tokio::test]
    async fn reachable_short_body_is_not_extractable() {
        let client = StubHttpClient::new([Ok(response(200, "Too short."))]);
        let result = probe_source(&client, "https://example.org/stub").await;
        assert!(result.reachable);
        assert!(!result.extractable);
        assert_eq!(result.unusable_reason, Some(BodyUsabilityReason::ShortBody));
        assert!(result.human_readable_hint);
    }

    #[tokio::test]
    async fn reachable_pdf_uses_the_full_verification_gate() {
        // A PDF served as application/pdf clears the text-shape gate (its bytes read as prose) but
        // the full usability gate rejects it as PdfBody — matching what verify_claim would return.
        // This is exactly the disagreement the probe must not have with the verification path.
        let client = StubHttpClient::new([Ok(response_with_content_type(
            200,
            "application/pdf",
            CLEAN_BODY,
        ))]);
        let result = probe_source(&client, "https://example.org/source.pdf").await;
        assert!(result.reachable);
        assert!(!result.extractable);
        assert_eq!(result.unusable_reason, Some(BodyUsabilityReason::PdfBody));
        assert!(result.human_readable_hint);
    }

    #[tokio::test]
    async fn reachable_viewer_shell_host_uses_the_full_verification_gate() {
        let client = StubHttpClient::new([Ok(response(200, CLEAN_BODY))]);
        let result = probe_source(&client, "https://books.google.com/books?id=abc").await;
        assert!(result.reachable);
        assert!(!result.extractable);
        assert_eq!(
            result.unusable_reason,
            Some(BodyUsabilityReason::ViewerShell)
        );
        assert!(result.human_readable_hint);
    }

    #[tokio::test]
    async fn non_2xx_is_unreachable_with_status() {
        let client = StubHttpClient::new([Ok(response(404, ""))]);
        let result = probe_source(&client, "https://example.org/gone").await;
        assert!(!result.reachable);
        assert_eq!(result.http_status, Some(404));
        assert!(!result.extractable);
        assert_eq!(result.unusable_reason, None);
        assert!(!result.human_readable_hint);
    }

    #[tokio::test]
    async fn transport_failure_is_unreachable_without_status() {
        let client = StubHttpClient::new([Err(HttpClientError::Transport {
            message: "connection refused".to_owned(),
        })]);
        let result = probe_source(&client, "https://example.org/down").await;
        assert!(!result.reachable);
        assert_eq!(result.http_status, None);
        assert!(!result.extractable);
        assert!(!result.human_readable_hint);
    }
}
