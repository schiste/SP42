//! Assessment-domain policy: GA-review-shaped rendering over the references
//! domain's verification reports (PRD-0016).
//!
//! The one export is a pure builder from `PageVerificationReport` to a plain
//! wikitext evidence appendix a Good-article reviewer pastes onto
//! `Talk:Article/GAn`. No I/O, no inference, no wiki writes.

pub mod copy;
pub mod ga_appendix;

pub use ga_appendix::render_ga_appendix;
