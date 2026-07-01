//! Dark/light theme state for the shared SP42 design system.
//!
//! Dark is the default. The browser choice persists in `localStorage` under
//! `sp42-theme` and is applied as a `data-theme` attribute on the document root.
//! Non-browser builds compile this API as a deterministic no-op so `sp42-ui`
//! remains a normal workspace crate for host checks.

use leptos::prelude::*;

/// Browser `localStorage` key used for the persisted design-system theme.
pub const THEME_STORAGE_KEY: &str = "sp42-theme";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Dark,
    Light,
}

impl Theme {
    /// Value written to `data-theme` / `localStorage`.
    #[must_use]
    pub const fn storage_value(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }

    /// The opposite theme; this is what the toggle switches to.
    #[must_use]
    pub const fn toggled(self) -> Self {
        match self {
            Self::Dark => Self::Light,
            Self::Light => Self::Dark,
        }
    }

    /// Label naming the theme this control switches to, not the current theme.
    #[must_use]
    pub const fn other_label(self) -> &'static str {
        match self {
            Self::Dark => "Light",
            Self::Light => "Dark",
        }
    }

    /// Accessible label for the theme toggle.
    #[must_use]
    pub const fn toggle_aria_label(self) -> &'static str {
        match self {
            Self::Dark => "Switch to light theme",
            Self::Light => "Switch to dark theme",
        }
    }

    /// Tooltip/title for the theme toggle.
    #[must_use]
    pub const fn toggle_title(self) -> &'static str {
        match self {
            Self::Dark => "Switch to Light theme",
            Self::Light => "Switch to Dark theme",
        }
    }
}

/// Reactive theme state restored from browser storage.
#[derive(Clone, Copy)]
pub struct ThemeState {
    current: ReadSignal<Theme>,
    set_current: WriteSignal<Theme>,
}

impl ThemeState {
    /// Current theme signal.
    #[must_use]
    pub const fn current(self) -> ReadSignal<Theme> {
        self.current
    }

    /// Switch to the opposite theme.
    pub fn toggle(self) {
        self.set_current
            .update(|current| *current = current.toggled());
    }
}

/// Restore persisted theme state and keep the document root synchronized.
#[must_use]
pub fn restore_theme() -> ThemeState {
    let (current, set_current) = signal(stored_theme());

    Effect::new(move |_| apply_theme(current.get()));

    ThemeState {
        current,
        set_current,
    }
}

/// Shared design-system theme toggle.
#[must_use]
pub fn theme_toggle(state: ThemeState) -> impl IntoView {
    view! {
        <button
            type="button"
            class="sp42-workspace-tab sp42-workspace-theme-toggle"
            on:click=move |_| state.toggle()
            aria-label=move || state.current().get().toggle_aria_label()
            title=move || state.current().get().toggle_title()
        >
            {move || state.current().get().other_label()}
        </button>
    }
}

pub use theme_toggle as ThemeToggle;

/// Theme saved from a previous browser session, or [`Theme::Dark`] by default.
#[must_use]
pub fn stored_theme() -> Theme {
    stored_theme_from_browser()
}

#[cfg(target_arch = "wasm32")]
fn stored_theme_from_browser() -> Theme {
    let stored = web_sys::window()
        .and_then(|window| window.local_storage().ok().flatten())
        .and_then(|storage| storage.get_item(THEME_STORAGE_KEY).ok().flatten());

    match stored.as_deref() {
        Some("light") => Theme::Light,
        _ => Theme::Dark,
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn stored_theme_from_browser() -> Theme {
    Theme::Dark
}

/// Apply `theme` to the document root (`data-theme`) and persist the choice.
pub fn apply_theme(theme: Theme) {
    apply_theme_to_browser(theme);
}

#[cfg(target_arch = "wasm32")]
fn apply_theme_to_browser(theme: Theme) {
    let Some(window) = web_sys::window() else {
        return;
    };

    if let Some(root) = window.document().and_then(|doc| doc.document_element()) {
        let _ = root.set_attribute("data-theme", theme.storage_value());
    }

    if let Ok(Some(storage)) = window.local_storage() {
        let _ = storage.set_item(THEME_STORAGE_KEY, theme.storage_value());
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn apply_theme_to_browser(_theme: Theme) {}

#[cfg(test)]
mod tests {
    use super::{Theme, stored_theme};

    #[test]
    fn toggled_is_involutive() {
        assert_eq!(Theme::Dark.toggled(), Theme::Light);
        assert_eq!(Theme::Light.toggled(), Theme::Dark);
        assert_eq!(Theme::Dark.toggled().toggled(), Theme::Dark);
    }

    #[test]
    fn other_label_names_the_target_theme() {
        assert_eq!(Theme::Dark.other_label(), "Light");
        assert_eq!(Theme::Light.other_label(), "Dark");
    }

    #[test]
    fn toggle_labels_name_the_target_theme() {
        assert_eq!(Theme::Dark.toggle_aria_label(), "Switch to light theme");
        assert_eq!(Theme::Light.toggle_aria_label(), "Switch to dark theme");
        assert_eq!(Theme::Dark.toggle_title(), "Switch to Light theme");
        assert_eq!(Theme::Light.toggle_title(), "Switch to Dark theme");
    }

    #[test]
    fn storage_value_matches_document_attribute_values() {
        assert_eq!(Theme::Dark.storage_value(), "dark");
        assert_eq!(Theme::Light.storage_value(), "light");
    }

    #[test]
    fn host_default_is_dark() {
        assert_eq!(stored_theme(), Theme::Dark);
    }
}
