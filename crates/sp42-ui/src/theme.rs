//! Dark/light theme state for the shared SP42 design system.
//!
//! Dark is the default. The browser choice persists in `localStorage` under
//! `sp42-theme` and is applied as a `data-theme` attribute on the document root.
//! Non-browser builds compile this API as a deterministic no-op so `sp42-ui`
//! remains a normal workspace crate for host checks.

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
}

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
    fn storage_value_matches_document_attribute_values() {
        assert_eq!(Theme::Dark.storage_value(), "dark");
        assert_eq!(Theme::Light.storage_value(), "light");
    }

    #[test]
    fn host_default_is_dark() {
        assert_eq!(stored_theme(), Theme::Dark);
    }
}
