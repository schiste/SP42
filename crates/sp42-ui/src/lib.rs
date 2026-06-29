#![forbid(unsafe_code)]

//! Shared Leptos presentation layer for SP42 shells.
//!
//! `sp42-ui` owns visual presentation: Codex-backed tokens, global CSS,
//! theming, and the typed primitives/patterns that will replace app-local
//! classes as the design-system migration proceeds. It intentionally has no
//! dependency on `sp42-app` or domain crates.

pub mod theme;

/// Trunk-bundled design-system stylesheet.
///
/// The runtime link is declared in `index.html`; this constant exists so Rust
/// code and tests can refer to the same owned asset without duplicating paths.
pub const DESIGN_SYSTEM_CSS: &str = include_str!("../static/style.css");

/// Repository-relative path to the design-system stylesheet.
pub const DESIGN_SYSTEM_CSS_PATH: &str = "crates/sp42-ui/static/style.css";

pub use theme::{
    THEME_STORAGE_KEY, Theme, ThemeState, ThemeToggle, apply_theme, restore_theme, stored_theme,
};
