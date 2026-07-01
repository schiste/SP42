//! Typed design-system primitives for SP42's Leptos UI.
//!
//! These functions own presentation choices through semantic variants. Callers
//! pass behavior, text, and children, but never raw CSS classes or inline style.

mod controls;
mod data_display;
mod diff;
mod feedback;
mod layout;
mod media;
mod navigation;
mod overlay;
mod typography;
mod util;

#[cfg(test)]
mod tests;

pub use controls::*;
pub use data_display::*;
pub use diff::*;
pub use feedback::*;
pub use layout::*;
pub use media::*;
pub use navigation::*;
pub use overlay::*;
pub use typography::*;
