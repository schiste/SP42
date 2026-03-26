//! UI components for the browser application.

pub(crate) mod action_bar;
pub(crate) mod context_sidebar;
pub(crate) mod diff_viewer;
pub(crate) mod filter_bar;
pub(crate) mod inspector_feed;
pub(crate) mod patrol_scenario_panel;
pub(crate) mod patrol_session_digest_panel;
pub(crate) mod queue_column;
pub(crate) mod shell_state_panel;
pub(crate) mod status_badge;
pub(crate) mod style;
pub(crate) mod telemetry_panel;

pub(crate) use inspector_feed::{InspectorFeed, inspector_entries_from_lines};
pub(crate) use patrol_scenario_panel::PatrolScenarioPanel;
pub(crate) use patrol_session_digest_panel::PatrolSessionDigestPanel;
pub(crate) use shell_state_panel::ShellStatePanel;
pub(crate) use status_badge::{StatusBadge, StatusTone};
pub(crate) use telemetry_panel::TelemetryPanel;
