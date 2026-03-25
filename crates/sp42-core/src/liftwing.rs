//! `LiftWing` request and response helpers.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use crate::errors::LiftWingError;
use crate::traits::HttpClient;
use crate::types::{HttpMethod, HttpRequest, WikiConfig};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiftWingRequest {
    pub rev_id: u64,
}

/// Build a `LiftWing` score request from a configured wiki.
///
/// # Errors
///
/// Returns [`LiftWingError`] when the revision ID is invalid or the wiki
/// configuration does not contain a `liftwing_url`.
pub fn build_liftwing_score_request(
    config: &WikiConfig,
    request: &LiftWingRequest,
) -> Result<HttpRequest, LiftWingError> {
    if request.rev_id == 0 {
        return Err(LiftWingError::InvalidRequest {
            message: "rev_id must be non-zero".to_string(),
        });
    }

    let url = config
        .liftwing_url
        .clone()
        .ok_or_else(|| LiftWingError::InvalidRequest {
            message: "liftwing_url is not configured".to_string(),
        })?;

    let body = serde_json::to_vec(&serde_json::json!({
        "rev_id": request.rev_id
    }))
    .map_err(LiftWingError::from)?;

    Ok(HttpRequest {
        method: HttpMethod::Post,
        url,
        headers: BTreeMap::from([("content-type".to_string(), "application/json".to_string())]),
        body,
    })
}

/// Execute a `LiftWing` request through the injected HTTP client.
///
/// # Errors
///
/// Returns [`LiftWingError`] when request construction fails, the HTTP client
/// returns an error, the HTTP status is not successful, or the response body
/// does not contain a supported probability field.
pub async fn execute_liftwing_score<C>(
    client: &C,
    config: &WikiConfig,
    request: &LiftWingRequest,
) -> Result<f32, LiftWingError>
where
    C: HttpClient + ?Sized,
{
    let http_request = build_liftwing_score_request(config, request)?;
    let response =
        client
            .execute(http_request)
            .await
            .map_err(|error| LiftWingError::InvalidResponse {
                message: error.to_string(),
            })?;

    if !(200..300).contains(&response.status) {
        return Err(LiftWingError::InvalidResponse {
            message: format!("unexpected HTTP status {}", response.status),
        });
    }

    parse_liftwing_score_response(&response.body)
}

/// Parse a probability score from a `LiftWing` JSON response.
///
/// # Errors
///
/// Returns [`LiftWingError`] when the response body is not valid JSON, does not
/// contain a supported probability field, or contains a probability outside the
/// `0.0..=1.0` range.
pub fn parse_liftwing_score_response(body: &[u8]) -> Result<f32, LiftWingError> {
    let parsed: Value = serde_json::from_slice(body).map_err(LiftWingError::from)?;

    if let Some(probability) = extract_probability_from_supported_shapes(&parsed) {
        return validate_probability(probability);
    }

    Err(LiftWingError::InvalidResponse {
        message: "response does not contain a supported probability field".to_string(),
    })
}

fn extract_probability_from_supported_shapes(value: &Value) -> Option<f32> {
    let object = value.as_object()?;

    direct_probability(object)
        .or_else(|| scores_probability(object))
        .or_else(|| output_probability(object))
        .or_else(|| prediction_list_probability(object.get("outputs")))
        .or_else(|| prediction_list_probability(object.get("predictions")))
}

fn direct_probability(object: &Map<String, Value>) -> Option<f32> {
    object.get("probability").and_then(value_as_f32)
}

fn scores_probability(object: &Map<String, Value>) -> Option<f32> {
    let scores = object.get("scores")?.as_object()?;
    scores
        .get("damaging")
        .and_then(value_as_f32)
        .or_else(|| scores.get("revertrisk").and_then(value_as_f32))
}

fn output_probability(object: &Map<String, Value>) -> Option<f32> {
    let output = object.get("output")?.as_object()?;
    let probabilities = output.get("probabilities")?.as_object()?;
    probabilities
        .get("true")
        .and_then(value_as_f32)
        .or_else(|| probabilities.get("damaging").and_then(value_as_f32))
}

fn prediction_list_probability(value: Option<&Value>) -> Option<f32> {
    value?
        .as_array()?
        .iter()
        .filter_map(Value::as_object)
        .find_map(|entry| {
            entry
                .get("probability")
                .and_then(value_as_f32)
                .or_else(|| entry.get("score").and_then(value_as_f32))
        })
}

fn value_as_f32(value: &Value) -> Option<f32> {
    match value {
        Value::Number(number) => number
            .to_string()
            .parse::<f32>()
            .ok()
            .filter(|p| p.is_finite()),
        _ => None,
    }
}

fn validate_probability(probability: f32) -> Result<f32, LiftWingError> {
    if !(0.0..=1.0).contains(&probability) {
        return Err(LiftWingError::InvalidResponse {
            message: format!("probability {probability} is outside 0.0..=1.0"),
        });
    }

    Ok(probability)
}
#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use futures::executor::block_on;
    use proptest::prelude::*;

    use super::{
        LiftWingRequest, build_liftwing_score_request, execute_liftwing_score,
        parse_liftwing_score_response,
    };
    use crate::config_parser::parse_wiki_config;
    use crate::traits::StubHttpClient;
    use crate::types::HttpResponse;

    const CONFIG: &str = include_str!("../../../configs/frwiki.yaml");

    #[test]
    fn builds_liftwing_request() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let request = build_liftwing_score_request(&config, &LiftWingRequest { rev_id: 123_456 })
            .expect("request should build");
        let body = String::from_utf8(request.body).expect("body should be utf-8");

        assert!(
            request
                .url
                .as_str()
                .contains("revertrisk-language-agnostic")
        );
        assert!(body.contains("\"rev_id\":123456"));
    }

    #[test]
    fn parses_direct_probability_shape() {
        let probability =
            parse_liftwing_score_response(br#"{"probability":0.91}"#).expect("should parse");

        assert!((probability - 0.91).abs() < f32::EPSILON);
    }

    #[test]
    fn parses_nested_probability_shape() {
        let probability =
            parse_liftwing_score_response(br#"{"output":{"probabilities":{"true":0.88}}}"#)
                .expect("should parse");

        assert!((probability - 0.88).abs() < f32::EPSILON);
    }

    #[test]
    fn parses_scores_revertrisk_shape() {
        let probability = parse_liftwing_score_response(br#"{"scores":{"revertrisk":0.42}}"#)
            .expect("should parse");

        assert!((probability - 0.42).abs() < f32::EPSILON);
    }

    #[test]
    fn parses_outputs_array_shape() {
        let probability = parse_liftwing_score_response(br#"{"outputs":[{"score":0.73}]}"#)
            .expect("should parse");

        assert!((probability - 0.73).abs() < f32::EPSILON);
    }

    #[test]
    fn parses_predictions_array_shape() {
        let probability =
            parse_liftwing_score_response(br#"{"predictions":[{"probability":0.64}]}"#)
                .expect("should parse");

        assert!((probability - 0.64).abs() < f32::EPSILON);
    }

    #[test]
    fn skips_empty_prediction_entries_until_supported_probability_is_found() {
        let probability =
            parse_liftwing_score_response(br#"{"predictions":[{"label":"ok"},{"score":0.44}]}"#)
                .expect("should parse");

        assert!((probability - 0.44).abs() < f32::EPSILON);
    }

    #[test]
    fn parses_string_probability_shape() {
        let error = parse_liftwing_score_response(br#"{"probability":"0.61"}"#)
            .expect_err("string probability should fail");

        assert!(error.to_string().contains("supported probability field"));
    }

    #[test]
    fn parses_nested_score_object_shape() {
        let error =
            parse_liftwing_score_response(br#"{"prediction":{"score":{"probability":0.58}}}"#)
                .expect_err("unsupported nested score object should fail");

        assert!(error.to_string().contains("supported probability field"));
    }

    #[test]
    fn rejects_probability_outside_unit_interval() {
        let error = parse_liftwing_score_response(br#"{"probability":1.4}"#)
            .expect_err("invalid probability should fail");

        assert!(error.to_string().contains("outside 0.0..=1.0"));
    }

    #[test]
    fn rejects_response_without_supported_probability_fields() {
        let error = parse_liftwing_score_response(br#"{"outputs":[{"label":"damaging"}]}"#)
            .expect_err("missing probability should fail");

        assert!(
            error
                .to_string()
                .contains("does not contain a supported probability field")
        );
    }

    #[test]
    fn rejects_missing_probability_shapes() {
        let error = parse_liftwing_score_response(br#"{"outputs":[{"label":"ok"}]}"#)
            .expect_err("missing probability should fail");

        assert!(error.to_string().contains("supported probability field"));
    }

    #[test]
    fn rejects_zero_revision_requests() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let error = build_liftwing_score_request(&config, &LiftWingRequest { rev_id: 0 })
            .expect_err("zero rev_id should fail");

        assert!(error.to_string().contains("rev_id must be non-zero"));
    }

    #[test]
    fn executes_liftwing_request_through_http_trait() {
        let config = parse_wiki_config(CONFIG).expect("config should parse");
        let client = StubHttpClient::new([Ok(HttpResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: br#"{"scores":{"damaging":0.67}}"#.to_vec(),
        })]);

        let probability = block_on(execute_liftwing_score(
            &client,
            &config,
            &LiftWingRequest { rev_id: 123_456 },
        ))
        .expect("liftwing execution should succeed");

        assert!((probability - 0.67).abs() < f32::EPSILON);
    }

    proptest! {
        #[test]
        fn property_direct_probability_shape_round_trips(probability in 0.0f32..=1.0f32) {
            let body = serde_json::json!({ "probability": probability }).to_string();
            let parsed = parse_liftwing_score_response(body.as_bytes()).expect("probability should parse");

            prop_assert!((parsed - probability).abs() < 0.000_001);
        }
    }
}
