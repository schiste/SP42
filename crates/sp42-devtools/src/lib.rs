#![forbid(unsafe_code)]

//! Deterministic SP42 developer fixtures, previews, and demo-surface builders.

pub mod preview;
pub mod surface;

pub use preview::{
    DEV_PREVIEW_ACTOR, DEV_PREVIEW_DEFAULT_CONFIG, DEV_PREVIEW_REV_ID,
    DEV_PREVIEW_SAMPLE_BACKLOG_RESPONSE, DEV_PREVIEW_SAMPLE_EVENTS, DEV_PREVIEW_WIKI_ID,
    DevBacklogPreview, DevCoordinationPreview, DevStreamPreview, build_dev_backlog_preview,
    build_dev_coordination_preview, build_dev_stream_preview, dev_coordination_message_label,
    dev_coordination_preview_messages, parse_default_dev_wiki_config,
};
pub use surface::{
    DevContextOptions, DevContextPreview, DevOperatorSurface, DevOperatorSurfaceOptions,
    DevWorkbenchOptions, DevtoolsError, build_default_dev_operator_surface,
    build_dev_action_requests, build_dev_context, build_dev_context_preview,
    build_dev_liftwing_request, build_dev_operator_surface, build_dev_queue,
    build_dev_recentchanges_request, build_dev_workbench, render_dev_backlog_preview,
    render_dev_coordination_preview, render_dev_queue_lines, render_dev_stream_actionable_lines,
    render_dev_stream_preview, render_dev_transport_lines,
};
