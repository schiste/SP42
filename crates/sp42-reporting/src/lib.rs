#![forbid(unsafe_code)]

//! SP42 **reporting framework** (platform): the domain-agnostic report document
//! model, the debug snapshot, and the server debug summary. Domain-specific
//! report *definitions* live in the owning domain crate: patrol reports in
//! `sp42-patrol`, citation findings/page reports in `sp42-citation` (ADR-0013).

pub mod debug_snapshot;
pub mod report_document;
pub mod server_debug_summary;

pub use debug_snapshot::{
    DebugSnapshot, DebugSnapshotInputs, DecisionTrace, PerformanceMarker, TraceLevel,
    build_debug_snapshot,
};
pub use report_document::{
    ReportDocument, ReportSection, render_report_document_markdown, render_report_document_text,
};
pub use server_debug_summary::ServerDebugSummary;
