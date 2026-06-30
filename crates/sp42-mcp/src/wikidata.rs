//! `verify_wikidata_statement` — the Wikidata convenience verb (PRD-0010, Phase 5).
//!
//! Net-new: there is no Wikidata path elsewhere in SP42. Given a statement reference, this fetches
//! the entity from Wikidata's keyless `Special:EntityData` endpoint, renders the statement into a
//! natural-language claim (resolving the property and any item-value to English labels via the
//! action API), extracts the statement's P854 *reference URL*, and verifies the claim against that
//! URL with [`verify_claim`]. A statement with no reference URL returns `SourceUnavailable` (not an
//! error) without any model call.
//!
//! Rendering is best-effort and intentionally minimal (PRD-0010 open question #1): the rendered
//! claim is always returned for inspection so a consumer can sanity-check it. Richer datatype
//! rendering is follow-on work.

use std::collections::BTreeMap;

use serde_json::Value;
use sp42_types::{Clock, HttpClient, HttpMethod, HttpRequest, ModelClient, ModelRef};

use crate::verify::verify_claim;
use crate::{Source, StatementRef, StatementVerifyResult, Verdict, VerifyResult};

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
    let entity_url = format!(
        "https://www.wikidata.org/wiki/Special:EntityData/{}.json",
        statement.entity
    );
    let entity_doc = get_json(fetch, &entity_url).await?;
    let parsed = parse_statement(&entity_doc, statement)?;

    // Resolve labels for the property and (if the value is an item) the value entity, in one call.
    let mut label_ids = vec![statement.property.clone()];
    if let Some(item) = &parsed.value_item {
        label_ids.push(item.clone());
    }
    let labels_url = format!(
        "https://www.wikidata.org/w/api.php?action=wbgetentities&ids={}&props=labels&languages=en&format=json",
        label_ids.join("|")
    );
    let labels = get_json(fetch, &labels_url).await.ok();

    let property_label = label_of(labels.as_ref(), &statement.property)
        .unwrap_or_else(|| statement.property.clone());
    let value_label = parsed
        .value_item
        .as_ref()
        .and_then(|item| label_of(labels.as_ref(), item))
        .unwrap_or_else(|| parsed.value_display.clone());
    let claim_rendered = format!(
        "{} {} {}.",
        parsed.subject_label, property_label, value_label
    );

    let Some(ref_url) = parsed.ref_url else {
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

/// The fields extracted from a single statement.
struct ParsedStatement {
    subject_label: String,
    /// The mainsnak value rendered to a display string (an item id when the value is an entity).
    value_display: String,
    /// The value entity id, when the mainsnak value is a `wikibase-entityid` (for label lookup).
    value_item: Option<String>,
    /// The first P854 reference URL on the statement, when present.
    ref_url: Option<String>,
}

/// GET `url` through the injected client and parse the body as JSON.
async fn get_json<C>(client: &C, url: &str) -> Result<Value, String>
where
    C: HttpClient + ?Sized,
{
    let request = HttpRequest {
        method: HttpMethod::Get,
        url: url.parse().map_err(|_| format!("invalid url {url:?}"))?,
        headers: BTreeMap::new(),
        body: Vec::new(),
    };
    let response = client
        .execute(request)
        .await
        .map_err(|error| error.to_string())?;
    if !(200..300).contains(&response.status) {
        return Err(format!("wikidata fetch {url} returned {}", response.status));
    }
    serde_json::from_slice(&response.body).map_err(|error| error.to_string())
}

/// Pull the subject label, statement value, and P854 reference URL out of an `EntityData` doc.
fn parse_statement(
    entity_doc: &Value,
    statement: &StatementRef,
) -> Result<ParsedStatement, String> {
    let entity = entity_doc
        .get("entities")
        .and_then(|entities| entities.get(&statement.entity))
        .ok_or_else(|| format!("entity {} not found", statement.entity))?;

    let subject_label = entity
        .get("labels")
        .and_then(|labels| labels.get("en"))
        .and_then(|en| en.get("value"))
        .and_then(Value::as_str)
        .unwrap_or(&statement.entity)
        .to_owned();

    let claims = entity
        .get("claims")
        .and_then(|claims| claims.get(&statement.property))
        .and_then(Value::as_array)
        .ok_or_else(|| {
            format!(
                "no statements for {} on {}",
                statement.property, statement.entity
            )
        })?;

    let chosen = match &statement.statement_id {
        Some(id) => claims
            .iter()
            .find(|stmt| stmt.get("id").and_then(Value::as_str) == Some(id.as_str()))
            .ok_or_else(|| format!("statement {id} not found"))?,
        None => claims
            .first()
            .ok_or_else(|| format!("no statements for {}", statement.property))?,
    };

    let (value_display, value_item) = chosen
        .get("mainsnak")
        .map_or_else(|| ("(no value)".to_owned(), None), render_value);

    Ok(ParsedStatement {
        subject_label,
        value_display,
        value_item,
        ref_url: extract_ref_url(chosen),
    })
}

/// Render a mainsnak's value to a display string, returning the entity id when it is an item.
fn render_value(mainsnak: &Value) -> (String, Option<String>) {
    let Some(datavalue) = mainsnak.get("datavalue") else {
        return ("(no value)".to_owned(), None);
    };
    let value = datavalue.get("value");
    match datavalue.get("type").and_then(Value::as_str).unwrap_or("") {
        "string" => (
            value.and_then(Value::as_str).unwrap_or_default().to_owned(),
            None,
        ),
        "wikibase-entityid" => {
            let id = value
                .and_then(|inner| inner.get("id"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let item = (!id.is_empty()).then(|| id.clone());
            (id, item)
        }
        "monolingualtext" => (
            value
                .and_then(|inner| inner.get("text"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            None,
        ),
        "time" => (
            value
                .and_then(|inner| inner.get("time"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim_start_matches('+')
                .to_owned(),
            None,
        ),
        "quantity" => (
            value
                .and_then(|inner| inner.get("amount"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim_start_matches('+')
                .to_owned(),
            None,
        ),
        _ => (value.map(ToString::to_string).unwrap_or_default(), None),
    }
}

/// The first P854 reference URL on the statement, when present.
fn extract_ref_url(statement: &Value) -> Option<String> {
    let references = statement.get("references")?.as_array()?;
    for reference in references {
        let p854 = reference
            .get("snaks")
            .and_then(|snaks| snaks.get("P854"))
            .and_then(Value::as_array);
        if let Some(snaks) = p854 {
            for snak in snaks {
                if let Some(url) = snak
                    .get("datavalue")
                    .and_then(|datavalue| datavalue.get("value"))
                    .and_then(Value::as_str)
                {
                    return Some(url.to_owned());
                }
            }
        }
    }
    None
}

/// Look up an English label for `id` in a `wbgetentities` labels response.
fn label_of(labels: Option<&Value>, id: &str) -> Option<String> {
    labels?
        .get("entities")?
        .get(id)?
        .get("labels")?
        .get("en")?
        .get("value")?
        .as_str()
        .map(str::to_owned)
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
