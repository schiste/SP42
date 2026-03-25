//! UI components for the browser application.

pub(crate) mod action_bar;
#[allow(dead_code)]
mod action_history_panel;
pub(crate) mod context_sidebar;
pub(crate) mod diff_viewer;
pub(crate) mod filter_bar;
pub(crate) mod inspector_feed;
pub(crate) mod patrol_scenario_panel;
pub(crate) mod patrol_session_digest_panel;
#[allow(dead_code)]
mod patrol_session_rail;
pub(crate) mod queue_column;
pub(crate) mod shell_state_panel;
pub(crate) mod status_badge;
#[allow(dead_code)]
mod telemetry_lines;
pub(crate) mod telemetry_panel;

#[allow(unused_imports)]
pub(crate) use action_history_panel::ActionHistoryPanel;
pub(crate) use inspector_feed::{InspectorFeed, inspector_entries_from_lines};
pub(crate) use patrol_scenario_panel::PatrolScenarioPanel;
pub(crate) use patrol_session_digest_panel::PatrolSessionDigestPanel;
#[allow(unused_imports)]
pub(crate) use patrol_session_rail::PatrolSessionRail;
pub(crate) use shell_state_panel::ShellStatePanel;
pub(crate) use status_badge::{StatusBadge, StatusTone};
#[allow(unused_imports)]
pub(crate) use telemetry_lines::{coordination_inspection_lines, server_debug_summary_lines};
pub(crate) use telemetry_panel::TelemetryPanel;
