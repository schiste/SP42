#![forbid(unsafe_code)]

//! Shared SP42 reporting models, summaries, digests, and renderers.

pub mod live_operator_view;
pub mod operator_summary;
pub mod patrol_scenario_report;
pub mod patrol_session_digest;
pub mod report_document;
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
pub use report_document::{
    ReportDocument, ReportSection, render_report_document_markdown, render_report_document_text,
};
pub use shell_state::{
    ShellPanelSummary, ShellStateInputs, ShellStateModel, ShellTimelineEntry, ShellTimelineStage,
    build_shell_state_model, render_shell_state_markdown, render_shell_state_text,
};
