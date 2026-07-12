//! `verify_wikidata_statement` — the Wikidata convenience verb (PRD-0010, Phase 5).
//!
//! Given a statement reference, this fetches the entity from Wikidata's keyless
//! `Special:EntityData` endpoint, renders the statement into a natural-language claim (resolving
//! the property and any item-value to English labels via the action API), extracts the
//! statement's P854 *reference URL*, and verifies the claim against that URL with
//! [`verify_claim`]. A statement with no reference URL returns `SourceUnavailable` (not an error)
//! without any model call.
//!
//! The entity/statement parsing and claim rendering live in the shared platform read model
//! (`sp42_platform::wikibase`, ADR-0016 Decision 2) — this verb was that model's first consumer
//! and now consumes it instead of carrying its own parser. P854 is the URL-reference case of the
//! platform's full reference snak set.
//!
//! Rendering is best-effort and intentionally minimal (PRD-0010 open question #1): the rendered
//! claim is always returned for inspection so a consumer can sanity-check it. Richer datatype
//! rendering is follow-on work.

use sp42_platform::wikibase::{
    WikibaseEntity, WikibaseLabels, build_entity_request, build_label_request, parse_entity,
    parse_labels, render_snak_value, render_statement_claim,
};
use sp42_types::{Clock, HttpClient, HttpRequest, ModelClient, ModelRef};
use url::Url;

use crate::verify::verify_claim;
use crate::{Source, StatementRef, StatementVerifyResult, Verdict, VerifyResult};

/// The language labels and claims are rendered in (PRD-0010 MVP scope).
const RENDER_LANGUAGE: &str = "en";

fn wikidata_api_url() -> Result<Url, String> {
    "https://www.wikidata.org/w/api.php"
        .parse()
        .map_err(|error| format!("wikidata api url did not parse: {error}"))
}

/// Verify a Wikidata statement against its P854 reference URL.
///
/// # Errors
///
/// Returns an error string if the entity cannot be fetched/parsed, the property has no statement,
/// or (when a reference URL is present) verification fails.
pub async fn verify_wikidata_statement<C, M>(
    fetch: &C,
    model: &M,
    clock: &dyn Clock,
    panel: &[ModelRef],
    statement: &StatementRef,
) -> Result<StatementVerifyResult, String>
where
    C: HttpClient + ?Sized,
    M: ModelClient + ?Sized,
{
    let api_url = wikidata_api_url()?;
    let entity_request = build_entity_request(&api_url, &statement.entity, None)
        .map_err(|error| error.to_string())?;
    let entity_body = fetch_ok(fetch, entity_request).await?;
    let entity =
        parse_entity(&statement.entity, &entity_body).map_err(|error| error.to_string())?;

    let chosen = entity
        .statement(&statement.property, statement.statement_id.as_deref())
        .ok_or_else(|| match &statement.statement_id {
            Some(id) => format!("statement {id} not found"),
            None => format!(
                "no statements for {} on {}",
                statement.property, statement.entity
            ),
        })?;

    // Resolve labels for the property and (if the value is an item) the value entity, in one
    // call. Label resolution is best-effort: a failed lookup falls back to raw ids.
    let mut label_ids = vec![statement.property.clone()];
    if let Some(item) = render_snak_value(&chosen.value).item {
        label_ids.push(item);
    }
    let labels = match build_label_request(&api_url, &label_ids, RENDER_LANGUAGE) {
        Ok(request) => match fetch_ok(fetch, request).await {
            Ok(body) => parse_labels(&body, RENDER_LANGUAGE).unwrap_or_default(),
            Err(_) => WikibaseLabels::default(),
        },
        Err(_) => WikibaseLabels::default(),
    };

    let claim_rendered = render_statement_claim(&subject_label(&entity), chosen, &labels);

    let ref_url = chosen
        .references
        .iter()
        .flat_map(sp42_platform::wikibase::WikibaseReference::urls)
        .next()
        .map(str::to_owned);
    let Some(ref_url) = ref_url else {
        // No reference URL: nothing to verify against. Abstain (no model call).
        return Ok(StatementVerifyResult {
            claim_rendered,
            ref_url: None,
            result: VerifyResult {
                verdict: Verdict::SourceUnavailable,
                quote: None,
                agreement: None,
            },
        });
    };

    let result = verify_claim(
        fetch,
        model,
        clock,
        panel,
        &claim_rendered,
        &Source::Url {
            url: ref_url.clone(),
        },
    )
    .await
    .map_err(|error| error.to_string())?;

    Ok(StatementVerifyResult {
        claim_rendered,
        ref_url: Some(ref_url),
        result,
    })
}

/// The entity's render-language label, falling back to its id.
fn subject_label(entity: &WikibaseEntity) -> String {
    entity
        .labels
        .get(RENDER_LANGUAGE)
        .cloned()
        .unwrap_or_else(|| entity.id.clone())
}

/// Execute a GET through the injected client, requiring a 2xx status.
async fn fetch_ok<C>(client: &C, request: HttpRequest) -> Result<Vec<u8>, String>
where
    C: HttpClient + ?Sized,
{
    let url = request.url.clone();
    let response = client
        .execute(request)
        .await
        .map_err(|error| error.to_string())?;
    if !(200..300).contains(&response.status) {
        return Err(format!("wikidata fetch {url} returned {}", response.status));
    }
    Ok(response.body)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use sp42_types::{
        FixedClock, HttpResponse, ModelCompletion, ModelRef, StubHttpClient, StubModelClient,
    };

    use super::verify_wikidata_statement;
    use crate::{StatementRef, Verdict};

    const BODY_TEXT: &str = "The Hitchhiker's Guide to the Galaxy is a comedy science-fiction \
        franchise created by Douglas Adams. Originally a radio comedy broadcast in 1978, it was \
        later adapted to other formats including novels, a television series, and a feature film, \
        becoming an international multimedia phenomenon over the following decades.";

    fn json_response(body: &str) -> HttpResponse {
        HttpResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: body.as_bytes().to_vec(),
        }
    }

    fn source_response(body: &str) -> HttpResponse {
        let mut headers = BTreeMap::new();
        headers.insert("content-type".to_owned(), "text/plain".to_owned());
        HttpResponse {
            status: 200,
            headers,
            body: body.as_bytes().to_vec(),
        }
    }

    fn entity_doc(with_reference: bool) -> String {
        let references = if with_reference {
            r#","references":[{"snaks":{"P854":[{"datavalue":{"value":"https://example.org/ref"}}]}}]"#
        } else {
            ""
        };
        format!(
            r#"{{"entities":{{"Q42":{{"labels":{{"en":{{"value":"Douglas Adams"}}}},"claims":{{"P800":[{{"id":"Q42$s1","mainsnak":{{"datavalue":{{"type":"string","value":"The Hitchhiker's Guide to the Galaxy"}}}}{references}}}]}}}}}}}}"#
        )
    }

    const LABELS_DOC: &str = r#"{"entities":{"P800":{"labels":{"en":{"value":"notable work"}}}}}"#;

    fn statement() -> StatementRef {
        StatementRef {
            entity: "Q42".to_owned(),
            property: "P800".to_owned(),
            statement_id: None,
        }
    }

    #[tokio::test]
    async fn renders_claim_and_verifies_against_reference_url() {
        let fetch = StubHttpClient::new([
            Ok(json_response(&entity_doc(true))),
            Ok(json_response(LABELS_DOC)),
            Ok(source_response(BODY_TEXT)),
        ]);
        let models = StubModelClient::new([Ok(ModelCompletion {
            text: r#"{"verdict": "SUPPORTED", "quote": "comedy science-fiction franchise created by Douglas Adams"}"#
                .to_owned(),
            served_model: None,
        })]);
        let result = verify_wikidata_statement(
            &fetch,
            &models,
            &FixedClock::new(0),
            &[ModelRef::new("test", "test-model", "test-model")],
            &statement(),
        )
        .await
        .expect("verifies");

        assert_eq!(
            result.claim_rendered,
            "Douglas Adams notable work The Hitchhiker's Guide to the Galaxy."
        );
        assert_eq!(result.ref_url.as_deref(), Some("https://example.org/ref"));
        assert_eq!(result.result.verdict, Verdict::Supported);
    }

    #[tokio::test]
    async fn statement_without_reference_url_is_source_unavailable() {
        // No P854 reference → no source fetch, no model call. Only entity + labels are fetched.
        let fetch = StubHttpClient::new([
            Ok(json_response(&entity_doc(false))),
            Ok(json_response(LABELS_DOC)),
        ]);
        let models = StubModelClient::new([]);
        let result = verify_wikidata_statement(
            &fetch,
            &models,
            &FixedClock::new(0),
            &[ModelRef::new("test", "test-model", "test-model")],
            &statement(),
        )
        .await
        .expect("renders");

        assert_eq!(result.ref_url, None);
        assert_eq!(result.result.verdict, Verdict::SourceUnavailable);
        // The rendered claim is still returned for inspection.
        assert!(result.claim_rendered.contains("Douglas Adams"));
    }

    #[tokio::test]
    async fn missing_entity_is_an_error() {
        let fetch = StubHttpClient::new([Ok(json_response(r#"{"entities":{}}"#))]);
        let models = StubModelClient::new([]);
        let result = verify_wikidata_statement(
            &fetch,
            &models,
            &FixedClock::new(0),
            &[ModelRef::new("test", "test-model", "test-model")],
            &statement(),
        )
        .await;
        assert!(result.is_err());
    }
}
