#![forbid(unsafe_code)]

//! SP42 **patrolling** domain: the patrol-specific policy, workflow, and report
//! definitions that sit on top of the platform's scoring engine/policy framework
//! and the reporting framework (ADR-0013).
//!
//! - scoring-evaluation fixture sets (regression / ranking / invariant / fairness)
//! - the patrol report definitions (operator summary, scenario report, session
//!   digest, shell state) built on the platform `report_document` framework
//! - the browser-facing live operator view
//!
//! The platform and reporting-framework surfaces are re-exported so the moved
//! modules' `crate::*` paths resolve unchanged after the split out of
//! `sp42-core` / `sp42-reporting`.

pub use sp42_platform::*;
pub use sp42_reporting::*;

pub mod live_operator_view;
pub mod operator_summary;
pub mod patrol_scenario_report;
pub mod patrol_session_digest;
pub mod scoring_evaluation;
pub mod shell_state;

pub use live_operator_view::LiveOperatorView;
pub use operator_summary::{
    PatrolOperatorSectionSummary, PatrolOperatorSummary, PatrolOperatorSummaryInputs,
    build_patrol_operator_summary, render_patrol_operator_summary_markdown,
    render_patrol_operator_summary_text,
};
pub use patrol_scenario_report::{
    PatrolScenarioFinding, PatrolScenarioReadiness, PatrolScenarioReport,
    PatrolScenarioReportInputs, PatrolScenarioSection, PatrolScenarioSelectedEdit, ReportSeverity,
    build_patrol_scenario_report, render_patrol_scenario_markdown, render_patrol_scenario_text,
};
pub use patrol_session_digest::{
    PatrolSessionDigest, PatrolSessionDigestInputs, PatrolSessionSectionSummary,
    PatrolSessionSelectedSummary, PatrolSessionSeverityCount, PatrolSessionWorkbenchSummary,
    build_patrol_session_digest, render_patrol_session_digest_markdown,
    render_patrol_session_digest_text,
};
pub use scoring_evaluation::{
    FairnessFixtureCheck, FairnessFixtureSet, InvariantFixtureRule, InvariantFixtureSet,
    RankingFixtureComparison, RankingFixtureSet, RegressionFixtureCase, RegressionFixtureSet,
    parse_fairness_fixture_set, parse_invariant_fixture_set, parse_ranking_fixture_set,
    parse_regression_fixture_set,
};
pub use shell_state::{
    ShellPanelSummary, ShellStateInputs, ShellStateModel, ShellTimelineEntry, ShellTimelineStage,
    build_shell_state_model, render_shell_state_markdown, render_shell_state_text,
};
