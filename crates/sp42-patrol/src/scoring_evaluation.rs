//! Typed scoring evaluation fixtures and profile helpers.

use serde::{Deserialize, Serialize};

use crate::errors::ScoringEvaluationError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegressionFixtureCase {
    pub slug: String,
    pub expectation: String,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegressionFixtureSet {
    pub cases: Vec<RegressionFixtureCase>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RankingFixtureComparison {
    pub higher: String,
    pub lower: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RankingFixtureSet {
    pub comparisons: Vec<RankingFixtureComparison>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvariantFixtureRule {
    pub slug: String,
    pub rule: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvariantFixtureSet {
    pub invariants: Vec<InvariantFixtureRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FairnessFixtureCheck {
    pub cohort: String,
    pub metric: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FairnessFixtureSet {
    pub checks: Vec<FairnessFixtureCheck>,
}

/// # Errors
///
/// Returns [`ScoringEvaluationError`] when the YAML cannot be decoded or the
/// fixture set is empty.
pub fn parse_regression_fixture_set(
    yaml: &str,
) -> Result<RegressionFixtureSet, ScoringEvaluationError> {
    let fixtures = serde_yaml::from_str::<RegressionFixtureSet>(yaml)?;
    if fixtures.cases.is_empty() {
        return Err(ScoringEvaluationError::InvalidField {
            field: "cases",
            message: "must not be empty".to_string(),
        });
    }
    Ok(fixtures)
}

/// # Errors
///
/// Returns [`ScoringEvaluationError`] when the YAML cannot be decoded or the
/// fixture set is empty.
pub fn parse_ranking_fixture_set(yaml: &str) -> Result<RankingFixtureSet, ScoringEvaluationError> {
    let fixtures = serde_yaml::from_str::<RankingFixtureSet>(yaml)?;
    if fixtures.comparisons.is_empty() {
        return Err(ScoringEvaluationError::InvalidField {
            field: "comparisons",
            message: "must not be empty".to_string(),
        });
    }
    Ok(fixtures)
}

/// # Errors
///
/// Returns [`ScoringEvaluationError`] when the YAML cannot be decoded or the
/// fixture set is empty.
pub fn parse_invariant_fixture_set(
    yaml: &str,
) -> Result<InvariantFixtureSet, ScoringEvaluationError> {
    let fixtures = serde_yaml::from_str::<InvariantFixtureSet>(yaml)?;
    if fixtures.invariants.is_empty() {
        return Err(ScoringEvaluationError::InvalidField {
            field: "invariants",
            message: "must not be empty".to_string(),
        });
    }
    Ok(fixtures)
}

/// # Errors
///
/// Returns [`ScoringEvaluationError`] when the YAML cannot be decoded or the
/// fixture set is empty.
pub fn parse_fairness_fixture_set(
    yaml: &str,
) -> Result<FairnessFixtureSet, ScoringEvaluationError> {
    let fixtures = serde_yaml::from_str::<FairnessFixtureSet>(yaml)?;
    if fixtures.checks.is_empty() {
        return Err(ScoringEvaluationError::InvalidField {
            field: "checks",
            message: "must not be empty".to_string(),
        });
    }
    Ok(fixtures)
}

#[cfg(test)]
mod tests {
    use super::{
        parse_fairness_fixture_set, parse_invariant_fixture_set, parse_ranking_fixture_set,
        parse_regression_fixture_set,
    };

    #[test]
    fn parses_regression_fixtures() {
        let fixtures = parse_regression_fixture_set(include_str!(
            "../../../evals/scoring/fixtures/vandalism_patrol/frwiki/regression.yaml"
        ))
        .expect("fixtures should parse");
        assert!(!fixtures.cases.is_empty());
    }

    #[test]
    fn parses_ranking_fixtures() {
        let fixtures = parse_ranking_fixture_set(include_str!(
            "../../../evals/scoring/fixtures/vandalism_patrol/frwiki/ranking.yaml"
        ))
        .expect("fixtures should parse");
        assert!(!fixtures.comparisons.is_empty());
    }

    #[test]
    fn parses_invariant_fixtures() {
        let fixtures = parse_invariant_fixture_set(include_str!(
            "../../../evals/scoring/fixtures/vandalism_patrol/frwiki/invariants.yaml"
        ))
        .expect("fixtures should parse");
        assert!(!fixtures.invariants.is_empty());
    }

    #[test]
    fn parses_fairness_fixtures() {
        let fixtures = parse_fairness_fixture_set(include_str!(
            "../../../evals/scoring/fixtures/vandalism_patrol/frwiki/fairness.yaml"
        ))
        .expect("fixtures should parse");
        assert!(!fixtures.checks.is_empty());
    }
}
